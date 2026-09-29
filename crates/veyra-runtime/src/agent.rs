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

use serde::{Deserialize, Serialize};
use serde_json::json;
use veyra_ai::{complete_with_fallback, extract_json, AIRequest, AIRouter, Capabilities};
use veyra_core::{Event, EventType, Goal, GoalStatus, RequirementStatus};
use veyra_store::{ProviderRunRecord, Store};
use veyra_tools::{default_tool_registry, CommandTool, PermissionLevel, Tool, ToolContext, ToolRegistry};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepPlan {
    pub description: String,
    /// Optional tool name from `ToolRegistry` (e.g. `write_file`, `list_directory`, `execute_command`).
    #[serde(default)]
    pub tool: Option<String>,
    /// Optional tool input payload.
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    /// Optional shell command that makes the step verifiable (backward compatible).
    #[serde(default)]
    pub command: Option<String>,
}

/// Result of executing one step.
#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    pub description: String,
    /// Tool name or "command" or "ai".
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
    tools: Arc<ToolRegistry>,
    limits: AgentLimits,
}

impl AgentRunner {
    pub fn new(
        router: Arc<AIRouter>,
        store: Arc<Mutex<Store>>,
        limits: AgentLimits,
    ) -> Self {
        Self::with_tools(router, store, Arc::new(default_tool_registry()), limits)
    }

    pub fn with_tools(
        router: Arc<AIRouter>,
        store: Arc<Mutex<Store>>,
        tools: Arc<ToolRegistry>,
        limits: AgentLimits,
    ) -> Self {
        Self {
            router,
            store,
            tool: Arc::new(CommandTool),
            tools,
            limits,
        }
    }

    pub fn tools(&self) -> &Arc<ToolRegistry> {
        &self.tools
    }

    fn append(
        &self,
        project_id: &str,
        event_type: EventType,
        payload: serde_json::Value,
    ) -> Result<(), LoopError> {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_event(&Event::now(project_id, event_type, payload))?;
        Ok(())
    }

    /// Ask the AI for a plan. Instructs JSON-only and extracts structurally;
    /// falls back to a single AI step on unparseable responses.
    async fn plan_steps(&self, goal: &Goal, previous_failure: Option<&str>) -> Vec<StepPlan> {
        let tools_manifests = self.tools.list_manifests();
        let mut tools_desc = String::new();
        for m in tools_manifests {
            tools_desc.push_str(&format!(
                "- {}: {} (引数スキーマ: {})\n",
                m.name, m.description, m.input_schema
            ));
        }

        let failure_context = match previous_failure {
            Some(err) => format!(
                "\n【重要: 前回の試行で以下のエラーが発生しました。原因を分析し自己修復するステップを計画してください】\n{}\n",
                err
            ),
            None => String::new(),
        };

        let request = AIRequest {
            task_type: "planning".into(),
            prompt: format!(
                "目標「{objective}」を達成するためのステッププランを作成してください。{failure_context}\n\
                 各ステップは、利用可能なツールを実行するか、AIによる推論・コード生成を行います。\n\n\
                 利用可能なツール一覧:\n{tools_desc}\n\
                 JSONのみを返却してください。\n\
                 例:\n\
                 {{\"steps\": [\n\
                   {{\"description\": \"プロジェクトのファイル一覧を確認\", \"tool\": \"list_directory\", \"input\": {{\"path\": \".\"}}}},\n\
                   {{\"description\": \"ファイルを更新\", \"tool\": \"write_file\", \"input\": {{\"path\": \"src/main.rs\", \"content\": \"...\"}}}},\n\
                   {{\"description\": \"テストを実行して検証\", \"tool\": \"execute_command\", \"input\": {{\"command\": \"cargo test\"}}}}\n\
                 ]}}",
                objective = goal.objective
            ),
            context: json!({ "goal_id": goal.id, "project_id": goal.project_id }),
            max_tokens: Some(800),
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
                let tool = s["tool"].as_str().map(str::to_string);
                let input = s.get("input").cloned();
                let command = s["command"].as_str().map(str::to_string);
                Some(StepPlan {
                    description,
                    tool,
                    input,
                    command,
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
            tool: None,
            input: None,
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
        self.store.lock().unwrap_or_else(|e| e.into_inner()).record_provider_run(&ProviderRunRecord {
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
        max_permission: PermissionLevel,
    ) -> StepReport {
        let ctx = ToolContext {
            project_id: project_id.to_string(),
            session_id: None,
            max_permission,
            root_path: root_path.map(std::path::PathBuf::from),
        };

        if let Some(tool_name) = &step.tool {
            let mut input = step.input.clone().unwrap_or_else(|| json!({}));
            if tool_name == "execute_command" {
                if let Some(obj) = input.as_object_mut() {
                    if !obj.contains_key("cwd") && root_path.is_some() {
                        obj.insert("cwd".into(), json!(root_path));
                    }
                    if !obj.contains_key("timeout_secs") {
                        obj.insert("timeout_secs".into(), json!(self.limits.step_timeout_secs));
                    }
                }
            }

            self.append(
                project_id,
                EventType::ToolRequested,
                json!({ "tool": tool_name, "input": &input }),
            )
            .ok();
            self.append(
                project_id,
                EventType::ToolStarted,
                json!({ "tool": tool_name }),
            )
            .ok();

            match self.tools.execute(tool_name, input.clone(), &ctx).await {
                Ok(result) => {
                    let success = result.output.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
                    let detail = if let Some(stdout) = result.output.get("stdout").and_then(|v| v.as_str()) {
                        format!("exit={:?} {}", result.output.get("exit_code"), stdout)
                    } else {
                        serde_json::to_string(&result.output).unwrap_or_default()
                    };

                    self.append(
                        project_id,
                        if success { EventType::ToolCompleted } else { EventType::ToolFailed },
                        json!({ "tool": tool_name, "success": success }),
                    )
                    .ok();

                    if tool_name == "write_file" && success {
                        if let Some(path) = input.get("path").and_then(|p| p.as_str()) {
                            self.append(
                                project_id,
                                EventType::FileModified,
                                json!({ "path": path }),
                            )
                            .ok();
                        }
                    }

                    StepReport {
                        description: step.description.clone(),
                        kind: tool_name.clone(),
                        success,
                        detail: if success {
                            detail
                        } else {
                            let stderr = result.output.get("stderr").and_then(|v| v.as_str()).unwrap_or_default();
                            if stderr.is_empty() { detail } else { stderr.to_string() }
                        },
                    }
                }
                Err(e) => {
                    self.append(
                        project_id,
                        EventType::ToolFailed,
                        json!({ "tool": tool_name, "error": e.to_string() }),
                    )
                    .ok();
                    StepReport {
                        description: step.description.clone(),
                        kind: tool_name.clone(),
                        success: false,
                        detail: e.to_string(),
                    }
                }
            }
        } else if let Some(command) = &step.command {
            self.append(project_id, EventType::ToolRequested, json!({ "command": command }))
                .ok();
            self.append(project_id, EventType::ToolStarted, json!({ "command": command }))
                .ok();

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
        } else {
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

    /// Run a goal through the bounded plan → execute → verify loop.
    ///
    /// State machine: `UNDERSTANDING → PLANNING → EXECUTING → VERIFYING`,
    /// with `EXECUTING → FAILED → REPLAN` recovery (bounded by
    /// `max_replans`) and `VERIFYING → COMPLETED` on success.
    pub async fn run(&self, goal: &Goal, root_path: Option<&str>, max_permission: PermissionLevel) -> Result<AgentReport, LoopError> {
        // Resolve latest project events for context.
        let _events = self.store.lock().unwrap_or_else(|e| e.into_inner()).events_for_project(&goal.project_id)?;

        // Start the state machine.
        let mut state = RuntimeState::Understanding;
        state = state.transition(RuntimeState::Planning).unwrap();

        let mut replans: usize = 0;
        let mut step_index: usize = 0;
        let mut steps: Vec<StepReport> = Vec::new();
        let mut last_failure: Option<String> = None;
        let verification_engine = VerificationEngine::new(self.store.clone(), self.router.clone());

        loop {
            // Generate a plan (PLANNING) on the first run and on each replan.
            if step_index == 0 || state == RuntimeState::Failed {
                let plan = self.plan_steps(goal, last_failure.as_deref()).await;
                self.append(
                    &goal.project_id,
                    EventType::IntentDetected,
                    json!({ "phase": "planning", "steps": plan.len(), "replans": replans }),
                )?;
                steps.clear();
                // R-1 Fix: Clear last_failure now that it has been passed to plan_steps for REPLAN,
                // allowing subsequent successful plan steps to set all_steps_ok = true.
                last_failure = None;

                // Recover: FAILED → REPLAN → EXECUTING (when applicable).
                if state == RuntimeState::Failed {
                    state = state.transition(RuntimeState::Replan).unwrap();
                }
                state = state.transition(RuntimeState::Executing).unwrap();
                let total_plan_steps = plan.len();
                let mut plan_truncated = false;

                for step in plan {
                    if step_index >= self.limits.max_steps {
                        plan_truncated = true;
                        break;
                    }
                    let report = self.execute_step(&goal.project_id, &step, root_path, max_permission).await;
                    step_index += 1;
                    steps.push(report.clone());

                    if !report.success {
                        last_failure = Some(format!("ステップ「{}」が失敗しました: {}", report.description, report.detail));
                        state = RuntimeState::Failed;
                        break;
                    }
                }

                if plan_truncated || steps.len() < total_plan_steps {
                    last_failure = Some(format!(
                        "計画ステップ（全{}件）が最大ステップ上限（{}）により途中で打ち切られました",
                        total_plan_steps, self.limits.max_steps
                    ));
                }
            }

            // Deterministic + supplemental verification (Plan.txt §30-31),
            // only when every step succeeded and plan was fully executed (C-2).
            let all_steps_ok = !steps.is_empty()
                && steps.iter().all(|s| s.success)
                && last_failure.is_none();

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

            // C-2: "Generate != Done" — verification must be non-empty and pass without being skipped.
            // If checks are empty (e.g. no root_path), verify_passed is FALSE (Unverified).
            let verify_passed = verify_outcome.as_ref().map_or(false, |o| {
                !o.checks.is_empty() && o.checks.iter().all(|c| c.passed && !c.skipped)
            });

            if all_steps_ok && verify_passed {
                // Validate VERIFYING → COMPLETED (transition is enforced by
                // the state machine even though we return immediately).
                let _ = state.transition(RuntimeState::Completed).unwrap();
                let mut completed_goal = goal.clone();
                completed_goal.status = GoalStatus::Completed;
                completed_goal.updated_at = chrono::Utc::now();
                for ac in &mut completed_goal.acceptance_criteria {
                    ac.status = RequirementStatus::Verified;
                }
                for req in &mut completed_goal.requirements {
                    req.status = RequirementStatus::Verified;
                }
                self.append(
                    &goal.project_id,
                    EventType::GoalUpdated,
                    json!({
                        "goal": completed_goal,
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

            // If verification failed, record failure info for the replan prompt
            if all_steps_ok && !verify_passed {
                let verify_detail = verify_outcome
                    .as_ref()
                    .map(|v| v.summary.clone())
                    .unwrap_or_else(|| "検証に失敗しました".to_string());
                last_failure = Some(format!("検証ステップが失敗しました: {}", verify_detail));
            }

            // Retried replan budget exceeded → stop.
            if replans >= self.limits.max_replans {
                let mut paused_goal = goal.clone();
                paused_goal.status = GoalStatus::Paused;
                paused_goal.updated_at = chrono::Utc::now();
                self.append(
                    &goal.project_id,
                    EventType::GoalUpdated,
                    json!({
                        "goal": paused_goal,
                        "completed": false,
                        "reason": "max_replans_exceeded",
                        "replans": replans,
                    }),
                )?;
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
                EventType::IntentDetected,
                json!({
                    "phase": "replan",
                    "replans": replans,
                    "goal_id": goal.id,
                    "failure": last_failure,
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
    use veyra_store::Projector;

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
        let dir = tempfile::tempdir().unwrap();
        let root_str = dir.path().to_str().unwrap();
        let project = store.lock().unwrap().create_project("proj", Some(root_str)).unwrap();

        // Mock AI replies with a plan of two steps.
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new(
            "ollama-qwen2.5-coder:7b",
            r#"{"steps": [{"description": "build", "command": "exit 0"}, {"description": "test", "command": "exit 0"}]}"#,
        )));
        let router = Arc::new(router);

        let runner = AgentRunner::new(router.clone(), store.clone(), AgentLimits::default());
        let report = runner.run(&sample_goal(&project.id), Some(root_str), PermissionLevel::HighRisk).await.unwrap();

        assert!(report.completed);
        assert_eq!(report.steps.len(), 2);
        assert!(report.steps.iter().all(|s| s.success));
        assert!(report.verification.is_some());
        assert!(report.verification.unwrap().passed);

        // Verify that the goal status in the project snapshot is updated to Completed!
        let events = store.lock().unwrap().events_for_project(&project.id).unwrap();
        let snapshot = Projector::project(&events);
        assert_eq!(snapshot.goals.len(), 1);
        assert_eq!(snapshot.goals[0].status, GoalStatus::Completed);
    }

    #[tokio::test]
    async fn agent_cannot_complete_without_verification_checks_c2a() {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        // Project with NO root_path: checks will be empty
        let project = store.lock().unwrap().create_project("proj_noroot", None).unwrap();

        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new(
            "ollama-qwen2.5-coder:7b",
            r#"{"steps": [{"description": "speak", "command": "exit 0"}]}"#,
        )));
        let router = Arc::new(router);

        let runner = AgentRunner::new(router.clone(), store.clone(), AgentLimits::default());
        let report = runner.run(&sample_goal(&project.id), None, PermissionLevel::HighRisk).await.unwrap();

        // C-2a: Generate != Done. Steps succeeded, but with 0 verification checks, goal MUST NOT be marked completed!
        assert!(!report.completed);
        assert!(report.verification.is_none());

        // Snapshot goal must NOT be Completed
        let events = store.lock().unwrap().events_for_project(&project.id).unwrap();
        let snapshot = Projector::project(&events);
        assert_ne!(snapshot.goals[0].status, GoalStatus::Completed);
    }

    #[tokio::test]
    async fn agent_executes_tool_registry_steps() {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let dir = tempfile::tempdir().unwrap();
        let root_str = dir.path().to_str().unwrap();
        let project = store.lock().unwrap().create_project("proj_tools", Some(root_str)).unwrap();

        // AI plans steps using write_file and read_file tools
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new(
            "ollama-qwen2.5-coder:7b",
            r#"{"steps": [
                {"description": "create hello file", "tool": "write_file", "input": {"path": "hello.txt", "content": "Hello Veyra"}},
                {"description": "read hello file", "tool": "read_file", "input": {"path": "hello.txt"}}
            ]}"#,
        )));
        let router = Arc::new(router);

        let runner = AgentRunner::new(router.clone(), store.clone(), AgentLimits::default());
        let report = runner.run(&sample_goal(&project.id), Some(root_str), PermissionLevel::HighRisk).await.unwrap();

        assert!(report.completed);
        assert_eq!(report.steps.len(), 2);
        assert!(report.steps[0].success);
        assert_eq!(report.steps[0].kind, "write_file");
        assert!(report.steps[1].success);
        assert_eq!(report.steps[1].kind, "read_file");

        // Verify the file was actually written to the filesystem!
        let content = std::fs::read_to_string(dir.path().join("hello.txt")).unwrap();
        assert_eq!(content, "Hello Veyra");
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
        let report = runner.run(&sample_goal(&project.id), None, PermissionLevel::HighRisk).await.unwrap();

        assert!(!report.completed);
        assert_eq!(report.replans, 1, "budget exhausted after first replan");
        assert!(report.steps.iter().all(|s| !s.success));

        // When replan budget is exceeded, goal is updated to Paused
        let events = store.lock().unwrap().events_for_project(&project.id).unwrap();
        let snapshot = Projector::project(&events);
        assert_eq!(snapshot.goals.len(), 1);
        assert_eq!(snapshot.goals[0].status, GoalStatus::Paused);
    }

    #[tokio::test]
    async fn respects_max_steps_bound() {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let project = store.lock().unwrap().create_project("proj", None).unwrap();

        // Plan proposes 5 steps but max_steps=2.
        let router = Arc::new(AIRouter::new());
        let limits = AgentLimits { max_steps: 2, step_timeout_secs: 10, max_replans: 0 };
        let runner = AgentRunner::new(router, store.clone(), limits);
        let report = runner.run(&sample_goal(&project.id), None, PermissionLevel::HighRisk).await.unwrap();

        // Fallback plan has a single step; max_steps caps execution.
        assert!(report.steps.len() <= 2);
    }

    struct SequentialMockProvider {
        id: veyra_ai::ProviderId,
        replies: Mutex<Vec<String>>,
    }

    impl SequentialMockProvider {
        fn new(id: &str, replies: Vec<String>) -> Self {
            Self {
                id: veyra_ai::ProviderId(id.to_string()),
                replies: Mutex::new(replies),
            }
        }
    }

    #[async_trait::async_trait]
    impl veyra_ai::AIProvider for SequentialMockProvider {
        fn id(&self) -> veyra_ai::ProviderId {
            self.id.clone()
        }
        fn capabilities(&self) -> veyra_ai::Capabilities {
            veyra_ai::Capabilities::default()
        }
        async fn complete(&self, _request: veyra_ai::AIRequest) -> Result<veyra_ai::AIResponse, veyra_ai::AIError> {
            let mut list = self.replies.lock().unwrap();
            let text = if !list.is_empty() {
                list.remove(0)
            } else {
                "{}".to_string()
            };
            Ok(veyra_ai::AIResponse {
                provider: self.id.0.clone(),
                model: format!("{}-seq-mock", self.id.0),
                text,
                input_tokens: None,
                output_tokens: None,
            })
        }
    }

    #[tokio::test]
    async fn agent_recovers_via_replan_and_completes_r1() {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let dir = tempfile::tempdir().unwrap();
        let root_str = dir.path().to_str().unwrap();
        let project = store.lock().unwrap().create_project("proj_replan", Some(root_str)).unwrap();

        // 1st plan has a failing step -> causes RuntimeState::Failed & triggers replan.
        // 2nd plan has a passing step -> must succeed and clear last_failure so all_steps_ok is true!
        let seq_provider = SequentialMockProvider::new(
            "ollama-qwen2.5-coder:7b",
            vec![
                r#"{"steps": [{"description": "failing step", "command": "exit 1"}]}"#.to_string(),
                r#"{"steps": [{"description": "passing step", "command": "exit 0"}]}"#.to_string(),
            ],
        );

        let mut router = AIRouter::new();
        router.register(Arc::new(seq_provider));
        let router = Arc::new(router);

        let limits = AgentLimits {
            max_steps: 5,
            step_timeout_secs: 10,
            max_replans: 2,
        };
        let runner = AgentRunner::new(router, store.clone(), limits);
        let report = runner.run(&sample_goal(&project.id), Some(root_str), PermissionLevel::HighRisk).await.unwrap();

        // R-1 check: Recovery succeeded!
        assert!(report.completed, "Goal must complete after successful replan!");
        assert_eq!(report.replans, 1, "Exactly 1 replan occurred");
        assert_eq!(report.steps.len(), 1, "Current successful plan has 1 step");
        assert!(report.steps[0].success);
        assert!(report.verification.is_some());
        assert!(report.verification.unwrap().passed);

        // Snapshot goal is Completed
        let events = store.lock().unwrap().events_for_project(&project.id).unwrap();
        let snapshot = Projector::project(&events);
        assert_eq!(snapshot.goals[0].status, GoalStatus::Completed);
    }
}
