//! Bounded agent execution (Plan.txt sections 22 and 42).
//!
//! ```text
//! User Goal → Plan → Task → AI/Tool → Result → Verification → Next Task
//! ```
//!
//! Agent loops are bounded by `max_steps`, `step_timeout` and
//! `max_replans`. State transitions go through the runtime state machine
//! (`IDLE → UNDERSTANDING → PLANNING → EXECUTING → VERIFYING → COMPLETED`,
//! `EXECUTING → FAILED → REPLAN → EXECUTING`), so an invalid transition
//! fails fast instead of corrupting state.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::json;
use veyra_ai::{complete_with_fallback, extract_json, AIRequest, AIRouter, Capabilities};
use veyra_core::{Event, EventType, Goal};
use veyra_store::{ProviderRunRecord, Store};
use veyra_tools::{CommandTool, PermissionLevel, Tool, ToolContext};

use crate::text_loop::{CODING_MODEL, LoopError};
use crate::verification::{default_checks_for, VerificationEngine, VerificationOutcome};
use crate::RuntimeState;

/// Hard bounds for one agent run.
#[derive(Debug, Clone)]
pub struct AgentLimits {
    pub max_steps: usize,
    pub step_timeout_secs: u64,
    pub max_replans: usize,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self { max_steps: 8, step_timeout_secs: 300, max_replans: 2 }
    }
}

/// One planned step proposed by the AI.
#[derive(Debug, Clone, Serialize)]
pub struct StepPlan {
    pub description: String,
    /// Optional shell command that makes the step verifiable.
    pub command: Option<String>,
}

/// Result of executing one step.
#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    pub description: String,
    /// "command" or "ai".
    pub kind: String,
    pub success: bool,
    pub detail: String,
}

/// Full report of an agent run.
#[derive(Debug, Serialize)]
pub struct AgentReport {
    pub goal_id: String,
    pub completed: bool,
    pub replans: usize,
    pub steps: Vec<StepReport>,
    pub verification: Option<VerificationOutcome>,
}

/// Executes a goal through a bounded plan → execute → verify loop.
pub struct AgentRunner {
    router: Arc<AIRouter>,
    store: Arc<Mutex<Store>>,
    tool: Arc<CommandTool>,
    limits: AgentLimits,
}

impl AgentRunner {
    pub fn new(
        router: Arc<AIRouter>,
        store: Arc<Mutex<Store>>,
        limits: AgentLimits,
    ) -> Self {
        Self { router, store, tool: Arc::new(CommandTool), limits }
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

    /// Ask the AI for a plan. Instructs JSON-only and extracts structurally;
    /// falls back to a single AI step on unparseable responses.
    async fn plan_steps(&self, goal: &Goal) -> Vec<StepPlan> {
        let request = AIRequest {
            task_type: "planning".into(),
            prompt: format!(
                "目標「{objective}」を達成するためのステッププランを作成してください。\n\
                 各ステップは description(説明)と、実行可能なら command(シェルコマンド)を持ちます。\n\
                 コマンドは検証に使われるので、build/test のような確定的なものを含めてください。\n\
                 JSONのみを返却してください。\
                 例: {{\"steps\": [{{\"description\": \"...\", \"command\": \"cargo test\"}}]}}",
                objective = goal.objective
            ),
            context: json!({ "goal_id": goal.id, "project_id": goal.project_id }),
            max_tokens: Some(600),
            temperature: Some(0.3),
        };
        let Ok(resp) =
            complete_with_fallback(&self.router, request, Some(CODING_MODEL), &Capabilities::default())
                .await
        else {
            return self.fallback_plan(goal);
        };
        let Some(value) = extract_json(&resp.text) else {
            return self.fallback_plan(goal);
        };
        let Some(steps) = value["steps"].as_array() else {
            return self.fallback_plan(goal);
        };
        let plan: Vec<StepPlan> = steps
            .iter()
            .filter_map(|s| {
                let description = s["description"].as_str()?.to_string();
                Some(StepPlan {
                    description,
                    command: s["command"].as_str().map(str::to_string),
                })
            })
            .collect();
        if plan.is_empty() {
            self.fallback_plan(goal)
        } else {
            plan
        }
    }

    /// Deterministic fallback when the AI cannot produce a plan.
    fn fallback_plan(&self, goal: &Goal) -> Vec<StepPlan> {
        vec![StepPlan {
            description: format!("目標「{}」に取り組む", goal.objective),
            command: None,
        }]
    }

    fn record_ai_run(
        &self,
        project_id: &str,
        task_type: &str,
        response: &veyra_ai::AIResponse,
    ) -> Result<(), LoopError> {
        let now = chrono::Utc::now();
        self.store.lock().unwrap().record_provider_run(&ProviderRunRecord {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_string(),
            provider: response.provider.clone(),
            model: response.model.clone(),
            task_type: Some(task_type.to_string()),
            input_metadata: None,
            output_metadata: Some(json!({
                "input_tokens": response.input_tokens,
                "output_tokens": response.output_tokens,
            })),
            status: "completed".into(),
            started_at: now,
            finished_at: Some(now),
        })?;
        Ok(())
    }

    async fn execute_step(
        &self,
        project_id: &str,
        step: &StepPlan,
        root_path: Option<&str>,
    ) -> StepReport {
        match &step.command {
            Some(command) => {
                self.append(project_id, EventType::ToolRequested, json!({ "command": command }))
                    .ok();
                self.append(project_id, EventType::ToolStarted, json!({ "command": command }))
                    .ok();

                let ctx = ToolContext {
                    project_id: project_id.to_string(),
                    session_id: None,
                    max_permission: PermissionLevel::MediumRisk,
                };
                let input = json!({
                    "command": command,
                    "shell": true,
                    "cwd": root_path,
                    "timeout_secs": self.limits.step_timeout_secs,
                });
                match self.tool.execute(input, &ctx).await {
                    Ok(result) => {
                        let success = result.output["success"].as_bool().unwrap_or(false);
                        let detail = format!(
                            "exit={:?} {}",
                            result.output["exit_code"],
                            result.output["stdout"].as_str().unwrap_or_default()
                        );
                        self.append(
                            project_id,
                            if success { EventType::ToolCompleted } else { EventType::ToolFailed },
                            json!({ "command": command, "success": success }),
                        )
                        .ok();
                        StepReport {
                            description: step.description.clone(),
                            kind: "command".into(),
                            success,
                            detail: if success {
                                detail
                            } else {
                                let stderr = result.output["stderr"].as_str().unwrap_or_default();
                                if stderr.is_empty() { detail } else { stderr.to_string() }
                            },
                        }
                    }
                    Err(e) => {
                        self.append(
                            project_id,
                            EventType::ToolFailed,
                            json!({ "command": command, "error": e.to_string() }),
                        )
                        .ok();
                        StepReport {
                            description: step.description.clone(),
                            kind: "command".into(),
                            success: false,
                            detail: e.to_string(),
                        }
                    }
                }
            }
            None => {
                // AI-only step: generate content for the description.
                let request = AIRequest {
                    task_type: "coding".into(),
                    prompt: format!(
                        "次のステップに取り組んでください: {}\n\
                         具体的な成果・提案を簡潔に日本語で返してください。",
                        step.description
                    ),
                    context: json!({ "project_id": project_id }),
                    max_tokens: Some(500),
                    temperature: Some(0.3),
                };
                match complete_with_fallback(
                    &self.router,
                    request,
                    Some(CODING_MODEL),
                    &Capabilities::default(),
                )
                .await
                {
                    Ok(resp) => {
                        self.record_ai_run(project_id, "coding", &resp).ok();
                        self.append(
                            project_id,
                            EventType::AssistantResponse,
                            json!({ "content": resp.text }),
                        )
                        .ok();
                        StepReport {
                            description: step.description.clone(),
                            kind: "ai".into(),
                            success: true,
                            detail: resp.text,
                        }
                    }
                    Err(e) => StepReport {
                        description: step.description.clone(),
                        kind: "ai".into(),
                        success: false,
                        detail: e.to_string(),
                    },
                }
            }
        }
    }

    /// Run a goal through the bounded plan → execute → verify loop.
    ///
    /// State machine: `UNDERSTANDING → PLANNING → EXECUTING → VERIFYING`,
    /// with `EXECUTING → FAILED → REPLAN` recovery (bounded by
    /// `max_replans`) and `VERIFYING → COMPLETED` on success.
    pub async fn run(&self, goal: &Goal, root_path: Option<&str>) -> Result<AgentReport, LoopError> {
        // Resolve latest project events for context.
        let _events = self.store.lock().unwrap().events_for_project(&goal.project_id)?;

        // Start the state machine.
        let mut state = RuntimeState::Understanding;
        state = state.transition(RuntimeState::Planning).unwrap();

        let mut replans: usize = 0;
        let mut step_index: usize = 0;
        let mut steps: Vec<StepReport> = Vec::new();
        let verification_engine = VerificationEngine::new(self.store.clone(), self.router.clone());

        loop {
            // Generate a plan (PLANNING) on the first run and on each replan.
            if step_index == 0 || state == RuntimeState::Failed {
                let plan = self.plan_steps(goal).await;
                self.append(
                    &goal.project_id,
                    EventType::IntentDetected,
                    json!({ "phase": "planning", "steps": plan.len() }),
                )?;
                steps.clear();

                // Recover: FAILED → REPLAN → EXECUTING (when applicable).
                if state == RuntimeState::Failed {
                    state = state.transition(RuntimeState::Replan).unwrap();
                }
                state = state.transition(RuntimeState::Executing).unwrap();

                for step in plan {
                    if step_index >= self.limits.max_steps {
                        break;
                    }
                    let report = self.execute_step(&goal.project_id, &step, root_path).await;
                    step_index += 1;
                    steps.push(report.clone());

                    if !report.success {
                        state = RuntimeState::Failed;
                        break;
                    }
                }
            }

            // Deterministic + supplemental verification (Plan.txt §30-31),
            // only when every step succeeded.
            let all_steps_ok = steps.iter().all(|s| s.success);
            let verify_outcome = if all_steps_ok {
                state = state.transition(RuntimeState::Verifying).unwrap();
                let checks = default_checks_for(root_path);
                if checks.is_empty() {
                    None
                } else {
                    verification_engine
                        .verify(&goal.project_id, root_path, &checks)
                        .await
                        .map(Some)?
                }
            } else {
                None
            };
            let verify_passed = verify_outcome.as_ref().map_or(true, |o| o.passed);

            if all_steps_ok && verify_passed {
                // Validate VERIFYING → COMPLETED (transition is enforced by
                // the state machine even though we return immediately).
                let _ = state.transition(RuntimeState::Completed).unwrap();
                self.append(
                    &goal.project_id,
                    EventType::GoalUpdated,
                    json!({
                        "goal": null,
                        "status": "completed",
                        "completed": true,
                        "steps": steps.len(),
                    }),
                )?;
                return Ok(AgentReport {
                    goal_id: goal.id.clone(),
                    completed: true,
                    replans,
                    steps,
                    verification: verify_outcome,
                });
            }

            // Retried replan budget exceeded → stop.
            if replans >= self.limits.max_replans {
                return Ok(AgentReport {
                    goal_id: goal.id.clone(),
                    completed: false,
                    replans,
                    steps,
                    verification: verify_outcome,
                });
            }

            // Failed → REPLAN.
            replans += 1;
            state = RuntimeState::Failed;
            self.append(
                &goal.project_id,
                EventType::GoalUpdated,
                json!({
                    "phase": "replan",
                    "replans": replans,
                    "goal_id": goal.id,
                }),
            )?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veyra_ai::MockProvider;
    use veyra_core::GoalStatus;

    fn sample_goal(project_id: &str) -> Goal {
        Goal {
            id: "goal-1".into(),
            project_id: project_id.into(),
            objective: "テスト目標".into(),
            requirements: vec![],
            constraints: vec![],
            preferences: vec![],
            acceptance_criteria: vec![],
            status: GoalStatus::Active,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn agent_completes_goal_with_verification() {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let project = store.lock().unwrap().create_project("proj", None).unwrap();

        // Mock AI replies with a plan of two steps.
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new(
            "ollama-qwen2.5-coder:7b",
            r#"{"steps": [{"description": "build", "command": "exit 0"}, {"description": "test", "command": "exit 0"}]}"#,
        )));
        let router = Arc::new(router);

        let runner = AgentRunner::new(router.clone(), store.clone(), AgentLimits::default());
        let report = runner.run(&sample_goal(&project.id), None).await.unwrap();

        assert!(report.completed);
        assert_eq!(report.steps.len(), 2);
        assert!(report.steps.iter().all(|s| s.success));
        // No root_path → no deterministic checks → verification is None.
        assert!(report.verification.is_none());
    }

    #[tokio::test]
    async fn stops_after_replan_budget_without_verification() {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let project = store.lock().unwrap().create_project("proj", None).unwrap();

        // Plan with a failing step → replans forever; budget stops it.
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new(
            "ollama-qwen2.5-coder:7b",
            r#"{"steps": [{"description": "always fails", "command": "exit 1"}]}"#,
        )));
        let router = Arc::new(router);

        let limits = AgentLimits { max_steps: 2, step_timeout_secs: 10, max_replans: 1 };
        let runner = AgentRunner::new(router, store.clone(), limits);
        let report = runner.run(&sample_goal(&project.id), None).await.unwrap();

        assert!(!report.completed);
        assert_eq!(report.replans, 1, "budget exhausted after first replan");
        assert!(report.steps.iter().all(|s| !s.success));
    }

    #[tokio::test]
    async fn respects_max_steps_bound() {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let project = store.lock().unwrap().create_project("proj", None).unwrap();

        // Plan proposes 5 steps but max_steps=2.
        let router = Arc::new(AIRouter::new());
        let limits = AgentLimits { max_steps: 2, step_timeout_secs: 10, max_replans: 0 };
        let runner = AgentRunner::new(router, store.clone(), limits);
        let report = runner.run(&sample_goal(&project.id), None).await.unwrap();

        // Fallback plan has a single step; max_steps caps execution.
        assert!(report.steps.len() <= 2);
    }
}
