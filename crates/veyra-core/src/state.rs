//! Project State describes the physical/current state of work
//! (Plan.txt sections 13 and 17).
//!
//! Goal answers "what to achieve"; Project State answers "how things are
//! right now". The two must never be mixed. Current state is a materialized
//! projection of the append-only event log and can be rebuilt after a crash.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Coarse phase of the current work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Planning,
    Executing,
    Verifying,
    Completed,
}

/// An issue blocking or affecting the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub detail: Option<String>,
}

/// Identity of a tracked file (Plan.txt section 29).
///
/// The content hash lets Veyra distinguish an expected modification (made
/// through Veyra) from an external modification (made outside Veyra).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    pub path: String,
    pub size: Option<u64>,
    pub modified_time: Option<DateTime<Utc>>,
    pub content_hash: Option<String>,
}

/// Result of a verification run (Plan.txt section 30).
///
/// Verification is deterministic first: "the app appears to work" is not a
/// verification result; "Build: PASS, Tests: 24/24 PASS" is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationResult {
    pub id: String,
    pub passed: bool,
    pub summary: String,
    pub verified_at: DateTime<Utc>,
}

/// The current, projected state of a project (Plan.txt section 13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectState {
    pub project_id: String,

    pub current_phase: Option<Phase>,
    pub current_task: Option<String>,

    pub completed_tasks: Vec<String>,
    pub active_tasks: Vec<String>,

    pub blocked_issues: Vec<Issue>,
    pub known_issues: Vec<Issue>,

    pub modified_files: Vec<FileRef>,

    pub last_verification: Option<VerificationResult>,

    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_state_round_trips() {
        let state = ProjectState {
            project_id: "proj-1".into(),
            current_phase: Some(Phase::Executing),
            current_task: Some("task-7".into()),
            completed_tasks: vec!["task-1".into(), "task-2".into()],
            active_tasks: vec!["task-7".into()],
            blocked_issues: vec![Issue {
                id: "issue-1".into(),
                title: "linker missing".into(),
                detail: None,
            }],
            known_issues: vec![],
            modified_files: vec![FileRef {
                path: "src/main.rs".into(),
                size: Some(1024),
                modified_time: Some(Utc::now()),
                content_hash: Some("sha256:abc".into()),
            }],
            last_verification: Some(VerificationResult {
                id: "verif-1".into(),
                passed: false,
                summary: "Build: PASS, Tests: 22/24 PASS".into(),
                verified_at: Utc::now(),
            }),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_value(&state).unwrap();
        let back: ProjectState = serde_json::from_value(json).unwrap();
        assert_eq!(back, state);
        assert!(!back.last_verification.as_ref().unwrap().passed);
    }
}
