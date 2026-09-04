//! Veyra desktop shell (Tauri 2).
//!
//! The shell is presentation-oriented: it exposes thin IPC commands over
//! the Rust authority layer (veyra-store / veyra-core). Business state
//! never lives only in the frontend.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{Manager, State};
use veyra_core::{Decision, Event, EventType, Goal, GoalStatus};
use veyra_store::{Project, Projector, Store};

/// Persistent state shared across IPC commands.
struct AppState {
    store: Mutex<Store>,
}

/// The current projection of a project, serialized for the UI.
#[derive(Serialize)]
struct SnapshotDto {
    goals: Vec<Goal>,
    decisions: Vec<Decision>,
    user_feedback: Vec<String>,
    last_verification: Option<veyra_core::VerificationResult>,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Store::open(data_dir.join("veyra.db"))?;
            app.manage(AppState {
                store: Mutex::new(store),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            project_snapshot,
            create_goal,
            add_user_feedback,
            record_decision
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
