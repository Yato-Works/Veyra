//! Veyra desktop shell (Tauri 2).
//!
//! The shell is presentation-oriented: it exposes thin IPC commands over
//! the Rust authority layer (veyra-store / veyra-core). Business state
//! never lives only in the frontend.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{Manager, State};
use veyra_ai::{AIRouter, OllamaProvider};
use veyra_core::{Decision, Event, EventType, Goal, GoalStatus};
use veyra_runtime::agent::{AgentLimits, AgentReport, AgentRunner};
use veyra_runtime::text_loop::{LoopOutcome, TextLoop, CODING_MODEL, INTENT_MODEL};
use veyra_runtime::verification::{
    VerificationEngine, VerificationOutcome, default_checks_for,
};
use veyra_store::{ChatMessage, Project, Projector, Store};

/// Persistent state shared across IPC commands.
struct AppState {
    store: Arc<Mutex<Store>>,
    router: Arc<AIRouter>,
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
        .unwrap()
        .list_projects()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn create_project(
    name: String,
    root_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Project, String> {
    state
        .store
        .lock()
        .unwrap()
        .create_project(&name, root_path.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn project_snapshot(project_id: String, state: State<'_, AppState>) -> Result<SnapshotDto, String> {
    let store = state.store.lock().unwrap();
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
        .unwrap()
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
        .unwrap()
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
        .unwrap()
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
    let text_loop = TextLoop::new(state.router.clone(), state.store.clone());
    text_loop
        .handle_input(&project_id, &text)
        .await
        .map_err(|e| e.to_string())
}

/// Execute a goal through the bounded plan → execute → verify loop (M4).
#[tauri::command]
async fn run_agent(
    goal_id: String,
    state: State<'_, AppState>,
) -> Result<AgentReport, String> {
    let (goal, root_path): (Goal, Option<String>) = {
        let store = state.store.lock().unwrap();
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

    let runner = AgentRunner::new(
        state.router.clone(),
        state.store.clone(),
        AgentLimits::default(),
    );
    runner
        .run(&goal, root_path.as_deref())
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
        let store = state.store.lock().unwrap();
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Store::open(data_dir.join("veyra.db"))?;

            // AI Router (Plan.txt sections 20-21): lightest local model for
            // intent, the coder model for coding, qwen3 as fallback.
            let mut router = AIRouter::new();
            router.register(Arc::new(OllamaProvider::new(INTENT_MODEL)));
            router.register(Arc::new(OllamaProvider::new(CODING_MODEL)));
            router.register(Arc::new(OllamaProvider::new("qwen3:4b")));

            app.manage(AppState {
                store: Arc::new(Mutex::new(store)),
                router: Arc::new(router),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            project_snapshot,
            create_goal,
            add_user_feedback,
            record_decision,
            send_message,
            run_agent,
            verify_project
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
