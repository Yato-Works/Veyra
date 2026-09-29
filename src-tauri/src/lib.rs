//! Veyra desktop shell (Tauri 2).
//!
//! The shell is presentation-oriented: it exposes thin IPC commands over
//! the Rust authority layer (veyra-store / veyra-core). Business state
//! never lives only in the frontend.

mod voice;

use std::sync::{Arc, Mutex};

use serde::Serialize;
use base64::Engine as _Base64Engine;
use tauri::{Emitter, Manager, State};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use veyra_ai::{AIRouter, OllamaProvider};
use veyra_core::{Decision, Event, EventType, Goal, GoalStatus};
use veyra_runtime::agent::{AgentLimits, AgentReport, AgentRunner};
use veyra_runtime::assistant::{AssistantReport, AssistantRunner};
use veyra_runtime::text_loop::{LoopOutcome, TextLoop, CODING_MODEL, INTENT_MODEL};
use veyra_runtime::verification::{
    VerificationEngine, VerificationOutcome, default_checks_for,
};
use veyra_store::{ChatMessage, Project, Projector, Store};
use veyra_tools::{default_tool_registry, PermissionLevel, ToolManifest, ToolRegistry};
use voice::{TranscriptPayload, VoiceRuntime};

/// Persistent state shared across IPC commands.
struct AppState {
    store: Arc<Mutex<Store>>,
    router: Arc<AIRouter>,
    tools: Arc<ToolRegistry>,
    assistant: Arc<AssistantRunner>,
    voice: Mutex<VoiceRuntime>,
}

/// The current projection of a project, serialized for the UI.
#[derive(Serialize)]
struct SnapshotDto {
    goals: Vec<Goal>,
    decisions: Vec<Decision>,
    user_feedback: Vec<String>,
    last_verification: Option<veyra_core::VerificationResult>,
    messages: Vec<ChatMessage>,
}

#[tauri::command]
fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    state
        .store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .list_projects()
        .map_err(|e| e.to_string())
}

/// Validates and normalizes an optional project root path.
/// Blocks system roots (e.g. C:, D:\), sensitive system directories, and control chars.
pub fn validate_project_root_path(path_str: &str) -> Result<String, String> {
    let trimmed_path = path_str.trim();
    if trimmed_path.is_empty() {
        return Err("Root path cannot be empty if specified".into());
    }
    let normalized = veyra_tools::normalize_path(std::path::Path::new(trimmed_path));
    let norm_slash = normalized.to_string_lossy().to_lowercase().replace('\\', "/");
    // Block dangerous root drives (C:, D:\, etc.) and filesystem root
    let is_drive_root = (norm_slash.len() == 2 && norm_slash.chars().nth(1) == Some(':') && norm_slash.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false))
        || (norm_slash.len() == 3 && norm_slash.chars().nth(1) == Some(':') && norm_slash.ends_with('/') && norm_slash.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false))
        || norm_slash == "/";
    if is_drive_root {
        return Err("Cannot set root path to system drive root".into());
    }
    let forbidden_roots = [
        "c:/windows",
        "c:/program files",
        "c:/program files (x86)",
        "/bin",
        "/sbin",
        "/etc",
        "/usr",
        "/var",
        "/sys",
        "/proc",
    ];
    for forbidden in forbidden_roots {
        if norm_slash.starts_with(forbidden) {
            return Err(format!(
                "Cannot set project root path inside protected system directory '{trimmed_path}'"
            ));
        }
    }
    Ok(normalized.to_string_lossy().to_string())
}

#[tauri::command]
fn create_project(
    name: String,
    root_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Project, String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("Project name cannot be empty".into());
    }
    if trimmed_name.len() > 100 {
        return Err("Project name cannot exceed 100 characters".into());
    }
    if trimmed_name.chars().any(|c| c.is_control()) {
        return Err("Project name cannot contain control characters".into());
    }

    let validated_root = match root_path {
        Some(ref p) => Some(validate_project_root_path(p)?),
        None => None,
    };

    state
        .store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .create_project(trimmed_name, validated_root.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn project_snapshot(project_id: String, state: State<'_, AppState>) -> Result<SnapshotDto, String> {
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    let events = store
        .events_for_project(&project_id)
        .map_err(|e| e.to_string())?;
    let snapshot = Projector::project(&events);
    Ok(SnapshotDto {
        goals: snapshot.goals,
        decisions: snapshot.decisions,
        user_feedback: snapshot.user_feedback,
        last_verification: snapshot.last_verification,
        messages: snapshot.messages,
    })
}

#[tauri::command]
fn create_goal(
    project_id: String,
    objective: String,
    state: State<'_, AppState>,
) -> Result<Goal, String> {
    let now = chrono::Utc::now();
    let goal = Goal {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.clone(),
        objective,
        requirements: vec![],
        constraints: vec![],
        preferences: vec![],
        acceptance_criteria: vec![],
        status: GoalStatus::Active,
        created_at: now,
        updated_at: now,
    };

    // Goal creation is expressed as an event: the projector rebuilds state.
    let event = Event::now(
        project_id,
        EventType::GoalCreated,
        serde_json::json!({ "goal": goal }),
    );
    state
        .store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .append_event(&event)
        .map_err(|e| e.to_string())?;
    Ok(goal)
}

#[tauri::command]
fn add_user_feedback(
    project_id: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let event = Event::now(
        project_id,
        EventType::UserFeedback,
        serde_json::json!({ "content": content }),
    );
    state
        .store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .append_event(&event)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn record_decision(
    project_id: String,
    decision: String,
    rationale: Option<String>,
    state: State<'_, AppState>,
) -> Result<Decision, String> {
    let record = Decision {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.clone(),
        decision,
        rationale,
        created_at: chrono::Utc::now(),
    };

    // Decision is not just chat history: it is a first-class event
    // that survives context compression (Plan.txt section 14).
    let event = Event::now(
        project_id,
        EventType::DecisionRecorded,
        serde_json::to_value(&record).map_err(|e| e.to_string())?,
    );
    state
        .store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .append_event(&event)
        .map_err(|e| e.to_string())?;
    Ok(record)
}

/// One turn of the M3 text loop: intent → goal/feedback → AI → event log.
#[tauri::command]
async fn send_message(
    project_id: String,
    text: String,
    state: State<'_, AppState>,
) -> Result<LoopOutcome, String> {
    let text_loop = TextLoop::with_tools(
        state.router.clone(),
        state.store.clone(),
        state.tools.clone(),
    );
    text_loop
        .handle_input(&project_id, &text)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Voice (M5)
// ---------------------------------------------------------------------------

/// Push-to-talk: start capturing from the default microphone.
#[tauri::command]
fn voice_start(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let mut voice = state.voice.lock().unwrap_or_else(|e| e.into_inner());
    if voice.recording.is_some() {
        return Err("already recording".into());
    }
    voice.recording = Some(veyra_voice::start_recording().map_err(|e| e.to_string())?);
    let _ = app.emit("voice-recording", true);
    Ok(())
}

/// Push-to-talk: stop capture, transcribe locally, clean up, return.
#[tauri::command]
async fn voice_stop(
    model: Option<String>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<TranscriptPayload, String> {
    let pcm = {
        let mut voice = state.voice.lock().unwrap_or_else(|e| e.into_inner());
        let handle = voice
            .recording
            .take()
            .ok_or_else(|| "not recording".to_string())?;
        handle.stop().map_err(|e| e.to_string())?
    };
    let _ = app.emit("voice-recording", false);
    voice::transcribe_public(&state, &app, pcm, model).await
}

/// Synthesize speech via Style-Bert-VITS2 and return base64 WAV.
#[tauri::command]
async fn speak(
    text: String,
    sbv2_url: Option<String>,
    model_name: Option<String>,
    style: Option<String>,
) -> Result<String, String> {
    let base = sbv2_url.unwrap_or_else(|| "http://127.0.0.1:5000".into());
    let client = veyra_voice::Sbv2Client::new(&base);
    let params = veyra_voice::Sbv2Params {
        model_name: model_name.unwrap_or_else(|| "jvnv-M1-jp".into()),
        style: style.unwrap_or_else(|| "Neutral".into()),
        ..Default::default()
    };
    let wav = client.synthesize(&text, &params).await.map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(wav))
}

/// Execute a goal through the bounded plan → execute → verify loop (M4).
#[tauri::command]
async fn run_agent(
    goal_id: String,
    permission_level: Option<PermissionLevel>,
    state: State<'_, AppState>,
) -> Result<AgentReport, String> {
    let (goal, root_path): (Goal, Option<String>) = {
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        // Goals live in the event log; scan projects to find the owner.
        let projects = store.list_projects().map_err(|e| e.to_string())?;
        let mut found: Option<(Goal, Option<String>)> = None;
        for project in projects {
            let events = store
                .events_for_project(&project.id)
                .map_err(|e| e.to_string())?;
            let snapshot = Projector::project(&events);
            if let Some(goal) = snapshot.goal(&goal_id) {
                found = Some((goal.clone(), project.root_path.clone()));
                break;
            }
        }
        found.ok_or_else(|| format!("goal not found: {goal_id}"))?
    };

    let runner = AgentRunner::with_tools(
        state.router.clone(),
        state.store.clone(),
        state.tools.clone(),
        AgentLimits::default(),
    );
    let perm = permission_level.unwrap_or(PermissionLevel::HighRisk);
    runner
        .run(&goal, root_path.as_deref(), perm)
        .await
        .map_err(|e| e.to_string())
}

/// Run the default verification checks against a project (M4).
#[tauri::command]
async fn verify_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<VerificationOutcome, String> {
    let root_path: Option<String> = {
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        let project = store
            .get_project(&project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        project.root_path.clone()
    };

    let checks = default_checks_for(root_path.as_deref());
    let engine = VerificationEngine::new(state.store.clone(), state.router.clone());
    engine
        .verify(&project_id, root_path.as_deref(), &checks)
        .await
        .map_err(|e| e.to_string())
}

/// Lists all registered tools and their manifests (M6).
#[tauri::command]
fn list_available_tools(state: State<'_, AppState>) -> Result<Vec<ToolManifest>, String> {
    Ok(state.tools.list_manifests())
}

/// Execute an assistant task using available tools and recording events (M6).
#[tauri::command]
async fn run_assistant_task(
    project_id: Option<String>,
    instruction: String,
    permission_level: Option<PermissionLevel>,
    state: State<'_, AppState>,
) -> Result<AssistantReport, String> {
    let pid = match project_id {
        Some(id) => id,
        None => {
            let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
            let projects = store.list_projects().map_err(|e| e.to_string())?;
            if let Some(first) = projects.first() {
                first.id.clone()
            } else {
                return Err("No project found. Please create a project before running assistant tasks.".into());
            }
        }
    };

    // Default to LowRisk unless explicitly requested
    let perm = permission_level.unwrap_or(PermissionLevel::LowRisk);

    state
        .assistant
        .run(&pid, &instruction, perm)
        .await
        .map_err(|e| e.to_string())
}

/// Cheap AI backend liveness probe for the UI status indicator.
#[tauri::command]
async fn ai_health() -> Result<bool, String> {
    Ok(veyra_ai::ollama_alive("http://localhost:11434").await)
}

// ---------------------------------------------------------------------------
// Overlay toggle: show/hide the window with animation trigger
// ---------------------------------------------------------------------------

/// Position window at the top-center of the current/primary monitor (Siri/Spotlight style).
fn position_top_center(window: &tauri::WebviewWindow) {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());

    if let Some(mon) = monitor {
        let mon_pos = mon.position();
        let mon_size = mon.size();
        let scale_factor = mon.scale_factor();
        if let Ok(window_size) = window.outer_size() {
            let x = mon_pos.x + ((mon_size.width as i32 - window_size.width as i32) / 2);
            let y = mon_pos.y + ((48.0 * scale_factor) as i32); // 48px from top (clears top bar)
            let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
        }
    }
}

/// Toggle the overlay window visibility. Called from the frontend or tray.
#[tauri::command]
fn toggle_overlay(app: tauri::AppHandle) -> Result<bool, String> {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        if visible {
            let _ = app.emit("overlay-hide", ());
            let _ = window.emit("overlay-hide", ());
            // Give the frontend time to animate out before hiding the window
            let win = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = win.hide();
            });
        } else {
            position_top_center(&window);
            let _ = window.unminimize();
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
            let _ = app.emit("overlay-show", ());
            let _ = window.emit("overlay-show", ());
        }
        Ok(!visible)
    } else {
        Err("main window not found".into())
    }
}

/// Toggle overlay visibility (called from Rust, e.g. tray/hotkey).
fn toggle_overlay_internal(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        if visible {
            let _ = app.emit("overlay-hide", ());
            let _ = window.emit("overlay-hide", ());
            let win = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = win.hide();
            });
        } else {
            position_top_center(&window);
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = app.emit("overlay-show", ());
            let _ = window.emit("overlay-show", ());
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let setup_result: Result<(), Box<dyn std::error::Error>> = (|| {
                let data_dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&data_dir)?;
                let store = Store::open(data_dir.join("veyra.db"))?;

                // AI Router (Plan.txt sections 20-21): lightest local model for
                // intent, the coder model for coding, qwen3 as fallback.
                let mut router = AIRouter::new();
                router.register(Arc::new(OllamaProvider::new(INTENT_MODEL)));
                router.register(Arc::new(OllamaProvider::new(CODING_MODEL)));
                router.register(Arc::new(OllamaProvider::new("qwen3:4b")));

                // Optional: Connect external OpenAI-compatible API (DeepSeek, OpenAI, Groq, LM Studio)
                if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
                    let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
                    router.register(Arc::new(veyra_ai::OpenAICompatibleProvider::new(model, base_url, Some(key))));
                }

                let store_arc = Arc::new(Mutex::new(store));
                let router_arc = Arc::new(router);
                let tools_arc = Arc::new(default_tool_registry());
                let assistant_arc = Arc::new(AssistantRunner::new(
                    router_arc.clone(),
                    store_arc.clone(),
                    tools_arc.clone(),
                ));

                app.manage(AppState {
                    store: store_arc,
                    router: router_arc,
                    tools: tools_arc,
                    assistant: assistant_arc,
                    voice: Mutex::new(VoiceRuntime { recording: None }),
                });

                // -----------------------------------------------------------
                // System Tray: Veyra lives in the tray as a resident app
                // -----------------------------------------------------------
                let show_i = MenuItem::with_id(app, "show", "Veyra を表示", true, None::<&str>)?;
                let hide_i = MenuItem::with_id(app, "hide", "隠す", true, None::<&str>)?;
                let quit_i = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

                // Always supply a valid icon so tray creation never fails
                static FALLBACK_ICON: [u8; 16 * 16 * 4] = [220; 16 * 16 * 4];
                let icon = app.default_window_icon().cloned().unwrap_or_else(|| {
                    tauri::image::Image::new(&FALLBACK_ICON, 16, 16)
                });

                let _tray = TrayIconBuilder::new()
                    .tooltip("Veyra – AI Companion")
                    .icon(icon)
                    .menu(&menu)
                    .on_menu_event(|app, event| {
                        match event.id().as_ref() {
                            "show" => {
                                if let Some(window) = app.get_webview_window("main") {
                                    position_top_center(&window);
                                    let _ = window.unminimize();
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                    let _ = app.emit("overlay-show", ());
                                    let _ = window.emit("overlay-show", ());
                                }
                            }
                            "hide" => {
                                if let Some(window) = app.get_webview_window("main") {
                                    let _ = app.emit("overlay-hide", ());
                                    let _ = window.emit("overlay-hide", ());
                                    let win = window.clone();
                                    std::thread::spawn(move || {
                                        std::thread::sleep(std::time::Duration::from_millis(300));
                                        let _ = win.hide();
                                    });
                                }
                            }
                            "quit" => {
                                std::process::exit(0);
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                            toggle_overlay_internal(tray.app_handle());
                        }
                    })
                    .build(app)?;

                // Automatically reveal overlay from top on initial startup
                if let Some(window) = app.get_webview_window("main") {
                    position_top_center(&window);
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = app.emit("overlay-show", ());
                    let _ = window.emit("overlay-show", ());
                }

                // Register global shortcuts safely: do not crash if already bound by another instance or app
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                for sc in ["ctrl+space", "ctrl+shift+space"] {
                    if let Err(e) = app.global_shortcut().register(sc) {
                        eprintln!("Warning: could not register global shortcut '{sc}': {e}");
                    }
                }

                Ok(())
            })();

            if let Err(ref e) = setup_result {
                let msg = format!("SETUP FAILED: {:?}\n", e);
                eprintln!("{}", msg);
                let _ = std::fs::write("C:\\Users\\smily\\Veyra\\setup_error.log", msg);
            }
            setup_result
        })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    let key = shortcut.into_string().to_lowercase();
                    let is_toggle = key.contains("shift"); // ctrl+shift+space
                    match event.state() {
                        ShortcutState::Pressed => {
                            if is_toggle {
                                // Ctrl+Shift+Space: toggle overlay visibility
                                toggle_overlay_internal(app);
                            } else {
                                // Ctrl+Space: push-to-talk (show overlay first if hidden)
                                if let Some(window) = app.get_webview_window("main") {
                                    if !window.is_visible().unwrap_or(false) {
                                        position_top_center(&window);
                                        let _ = window.unminimize();
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                        let _ = app.emit("overlay-show", ());
                                        let _ = window.emit("overlay-show", ());
                                    }
                                }
                                voice::on_hotkey_press(app);
                            }
                        }
                        ShortcutState::Released => {
                            if !is_toggle {
                                // Ctrl+Space release: stop recording
                                voice::on_hotkey_release(app);
                            }
                        }
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            project_snapshot,
            create_goal,
            add_user_feedback,
            record_decision,
            send_message,
            run_agent,
            verify_project,
            list_available_tools,
            run_assistant_task,
            voice_start,
            voice_stop,
            speak,
            toggle_overlay,
            ai_health
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_valid_project_root_path() {
        let valid = validate_project_root_path("C:\\Users\\smily\\Projects\\MyApp");
        assert!(valid.is_ok());
    }

    #[test]
    fn blocks_system_drive_roots() {
        assert!(validate_project_root_path("C:\\").is_err());
        assert!(validate_project_root_path("c:").is_err());
        assert!(validate_project_root_path("D:\\").is_err());
        assert!(validate_project_root_path("d:").is_err());
        assert!(validate_project_root_path("/").is_err());
        assert!(validate_project_root_path("\\").is_err());
    }

    #[test]
    fn blocks_protected_system_directories() {
        assert!(validate_project_root_path("C:\\Windows\\System32").is_err());
        assert!(validate_project_root_path("C:\\Program Files\\App").is_err());
        assert!(validate_project_root_path("C:\\Program Files (x86)\\App").is_err());
        assert!(validate_project_root_path("/etc/config").is_err());
        assert!(validate_project_root_path("/sys/kernel").is_err());
    }

    #[test]
    fn blocks_empty_root_path() {
        assert!(validate_project_root_path("").is_err());
        assert!(validate_project_root_path("   ").is_err());
    }

    #[test]
    fn test_shortcut_matching() {
        use std::str::FromStr;
        use tauri_plugin_global_shortcut::Shortcut;
        let s1 = Shortcut::from_str("ctrl+shift+space").unwrap();
        let s2 = Shortcut::from_str("ctrl+space").unwrap();
        println!("s1: {:?}, into_string: {}", s1, s1.into_string());
        println!("s2: {:?}, into_string: {}", s2, s2.into_string());
        let s1_str = s1.into_string().to_lowercase();
        assert!(s1_str.contains("shift"));
        let s2_str = s2.into_string().to_lowercase();
        assert!(!s2_str.contains("shift"));
    }
}
