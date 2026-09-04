//! Verification Engine — a first-class Runtime component
//! (Plan.txt sections 30-31).
//!
//! Prefer deterministic verification over judgment:
//!
//! ```text
//! Deterministic → Programmatic → Visual → LLM Judgment
//! ```
//!
//! "The app appears to work" is not a verification result;
//! "Build: PASS, Tests: 24/24 PASS" is. LLM review is supplemental:
//! when the AI is unavailable the check is *skipped*, never fabricated.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::json;
use veyra_ai::{complete_with_fallback, extract_json, AIRequest, AIRouter, Capabilities};
use veyra_core::{Event, EventType, VerificationResult};
use veyra_store::Store;
use veyra_tools::{CommandTool, PermissionLevel, Tool, ToolContext};

use crate::text_loop::LoopError;

/// One configured verification check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckSpec {
    /// Run a command (build / test / lint) — deterministic.
    Command { name: Option<String>, command: String },
    /// Check that a file or directory exists — deterministic.
    FileExists { name: Option<String>, path: String },
    /// Ask an AI to review against the goal — supplemental judgment.
    AiReview { name: Option<String>, instruction: String },
}

/// Result of a single check.
#[derive(Debug, Clone, Serialize)]
pub struct CheckOutcome {
    pub name: String,
    pub passed: bool,
    /// true when the check could not be performed (e.g. AI unavailable).
    pub skipped: bool,
    pub detail: String,
}

/// Result of a full verification run.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationOutcome {
    pub passed: bool,
    pub summary: String,
    pub checks: Vec<CheckOutcome>,
}

/// Executes verification checks and records them in the event log.
pub struct VerificationEngine {
    store: Arc<Mutex<Store>>,
    router: Arc<AIRouter>,
    tool: Arc<CommandTool>,
}

impl VerificationEngine {
    pub fn new(store: Arc<Mutex<Store>>, router: Arc<AIRouter>) -> Self {
        Self { store, router, tool: Arc::new(CommandTool) }
    }

    fn append(
        &self,
        project_id: &str,
        event_type: EventType,
        payload: serde_json::Value,
    ) -> Result<(), LoopError> {
        self.store
            .lock()
            .unwrap()
            .append_event(&Event::now(project_id, event_type, payload))?;
        Ok(())
    }

    /// Run all checks, record `verification_started` / `verification_completed`
    /// events, and return the outcome. A run passes only when every
    /// non-skipped check passed.
    pub async fn verify(
        &self,
        project_id: &str,
        root_path: Option<&str>,
        checks: &[CheckSpec],
    ) -> Result<VerificationOutcome, LoopError> {
        self.append(
            project_id,
            EventType::VerificationStarted,
            json!({ "checks": checks.len() }),
        )?;

        let mut outcomes = Vec::new();
        for check in checks {
            outcomes.push(self.run_check(project_id, root_path, check).await);
        }

        let passed = outcomes.iter().all(|o| o.passed || o.skipped);
        let summary = outcomes
            .iter()
            .map(|o| {
                let mark = if o.skipped {
                    "SKIP"
                } else if o.passed {
                    "PASS"
                } else {
                    "FAIL"
                };
                format!("{}: {mark}", o.name)
            })
            .collect::<Vec<_>>()
            .join(", ");

        let result = VerificationResult {
            id: uuid::Uuid::new_v4().to_string(),
            passed,
            summary: summary.clone(),
            verified_at: chrono::Utc::now(),
        };
        self.append(
            project_id,
            EventType::VerificationCompleted,
            serde_json::to_value(&result)?,
        )?;

        Ok(VerificationOutcome { passed, summary, checks: outcomes })
    }

    async fn run_check(
        &self,
        project_id: &str,
        root_path: Option<&str>,
        check: &CheckSpec,
    ) -> CheckOutcome {
        match check {
            CheckSpec::Command { name, command } => {
                let name = name.clone().unwrap_or_else(|| command.clone());
                let ctx = ToolContext {
                    project_id: project_id.to_string(),
                    session_id: None,
                    max_permission: PermissionLevel::MediumRisk,
                };
                let input = json!({
                    "command": command,
                    "shell": true,
                    "cwd": root_path,
                    "timeout_secs": 300,
                });
                match self.tool.execute(input, &ctx).await {
                    Ok(result) => {
                        let success = result.output["success"].as_bool().unwrap_or(false);
                        let detail = if success {
                            result.output["stdout"].as_str().unwrap_or_default().to_string()
                        } else {
                            let stderr = result.output["stderr"].as_str().unwrap_or_default();
                            let stdout = result.output["stdout"].as_str().unwrap_or_default();
                            if stderr.is_empty() { stdout.to_string() } else { stderr.to_string() }
                        };
                        CheckOutcome { name, passed: success, skipped: false, detail }
                    }
                    Err(e) => CheckOutcome {
                        name,
                        passed: false,
                        skipped: false,
                        detail: e.to_string(),
                    },
                }
            }
            CheckSpec::FileExists { name, path } => {
                let name = name.clone().unwrap_or_else(|| format!("exists: {path}"));
                match root_path {
                    Some(root) => {
                        let target = std::path::Path::new(root).join(path);
                        CheckOutcome {
                            passed: target.exists(),
                            skipped: false,
                            detail: target.display().to_string(),
                            name,
                        }
                    }
                    None => CheckOutcome {
                        name,
                        passed: false,
                        skipped: true,
                        detail: "no root_path configured".into(),
                    },
                }
            }
            CheckSpec::AiReview { name, instruction } => {
                let name = name.clone().unwrap_or_else(|| "AI review".into());
                let request = AIRequest {
                    task_type: "verification".into(),
                    prompt: format!(
                        "{instruction}\n\n検証対象のプロジェクト判断として、\
                         成功/失敗を JSON で返却してください。\
                         例: {{\"passed\": true, \"reason\": \"...\"}}"
                    ),
                    context: json!({ "project_id": project_id }),
                    max_tokens: Some(200),
                    temperature: Some(0.2),
                };
                match complete_with_fallback(&self.router, request, None, &Capabilities::default())
                    .await
                {
                    Ok(resp) => match extract_json(&resp.text) {
                        Some(v) => CheckOutcome {
                            name,
                            passed: v["passed"].as_bool().unwrap_or(false),
                            skipped: false,
                            detail: v["reason"].as_str().unwrap_or_default().to_string(),
                        },
                        None => CheckOutcome {
                            name,
                            passed: false,
                            skipped: true,
                            detail: "AI response contained no JSON".into(),
                        },
                    },
                    Err(e) => CheckOutcome {
                        name,
                        passed: false,
                        skipped: true,
                        detail: format!("AI unavailable: {e}"),
                    },
                }
            }
        }
    }
}

/// Sensible default checks for a project root (Plan.txt section 30).
///
/// Rust projects get deterministic build + test checks; roots without a
/// Cargo.toml fall back to verifying the root directory exists.
pub fn default_checks_for(root_path: Option<&str>) -> Vec<CheckSpec> {
    let Some(root) = root_path else {
        return vec![];
    };
    if std::path::Path::new(root).join("Cargo.toml").exists() {
        vec![
            CheckSpec::Command {
                name: Some("Build (cargo check)".into()),
                command: "cargo check".into(),
            },
            CheckSpec::Command {
                name: Some("Tests (cargo test)".into()),
                command: "cargo test".into(),
            },
        ]
    } else {
        vec![CheckSpec::FileExists { name: None, path: ".".into() }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn engine() -> (Arc<Mutex<Store>>, String, VerificationEngine) {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let project = store.lock().unwrap().create_project("proj", None).unwrap();
        let engine = VerificationEngine::new(store.clone(), Arc::new(AIRouter::new()));
        (store, project.id, engine)
    }

    #[tokio::test]
    async fn command_check_passes_and_fails() {
        let (store, project_id, engine) = engine();

        let ok = engine
            .verify(&project_id, None, &[CheckSpec::Command {
                name: Some("always passes".into()),
                command: "exit 0".into(),
            }])
            .await
            .unwrap();
        assert!(ok.passed);

        let bad = engine
            .verify(&project_id, None, &[CheckSpec::Command {
                name: Some("always fails".into()),
                command: "exit 1".into(),
            }])
            .await
            .unwrap();
        assert!(!bad.passed);

        // Both runs were recorded in the event log.
        let events = store.lock().unwrap().events_for_project(&project_id).unwrap();
        let started = events
            .iter()
            .filter(|e| e.event_type == EventType::VerificationStarted)
            .count();
        let completed = events
            .iter()
            .filter(|e| e.event_type == EventType::VerificationCompleted)
            .count();
        assert_eq!((started, completed), (2, 2));
    }

    #[tokio::test]
    async fn file_check_skips_without_root() {
        let (_, project_id, engine) = engine();
        let outcome = engine
            .verify(&project_id, None, &[CheckSpec::FileExists {
                name: None,
                path: "Cargo.toml".into(),
            }])
            .await
            .unwrap();
        assert!(outcome.passed, "skipped checks must not fail the run");
        assert!(outcome.checks[0].skipped);
    }

    #[tokio::test]
    async fn file_check_fails_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let (_, project_id, engine) = engine();
        let outcome = engine
            .verify(
                &project_id,
                Some(dir.path().to_str().unwrap()),
                &[CheckSpec::FileExists {
                    name: None,
                    path: "definitely-missing.txt".into(),
                }],
            )
            .await
            .unwrap();
        assert!(!outcome.passed);
    }

    #[tokio::test]
    async fn ai_review_is_skipped_when_ai_unavailable() {
        let (_, project_id, engine) = engine();
        let outcome = engine
            .verify(&project_id, None, &[CheckSpec::AiReview {
                name: Some("review".into()),
                instruction: "レビューしてください".into(),
            }])
            .await
            .unwrap();
        assert!(outcome.checks[0].skipped);
        assert!(outcome.passed, "skipped AI review must not fail the run");
    }

    #[test]
    fn default_checks_detect_rust_projects() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let checks = default_checks_for(Some(dir.path().to_str().unwrap()));
        assert_eq!(checks.len(), 2);
        assert!(matches!(checks[0], CheckSpec::Command { .. }));

        assert!(default_checks_for(None).is_empty());
    }
}
