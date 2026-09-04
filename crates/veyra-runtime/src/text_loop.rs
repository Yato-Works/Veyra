//! M3 text loop: input → intent → goal/feedback resolution → AI execution
//! → event log (Plan.txt sections 2, 32, 41).
//!
//! Mirrors the proven two-tier pattern from AIStoryActingEngine: the LLM
//! classifies intent, and a deterministic keyword rule set takes over
//! automatically when the LLM is unavailable or returns garbage.

use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use serde_json::json;
use veyra_ai::{complete_with_fallback, extract_json, AIRequest, AIRouter, Capabilities};
use veyra_core::{Event, EventType, Goal, GoalStatus};
use veyra_store::{Projector, ProviderRunRecord, Store};

/// Preferred provider id for intent classification (lightest local model).
pub const INTENT_MODEL: &str = "qwen2.5:1.5b";
/// Preferred provider id for coding tasks.
pub const CODING_MODEL: &str = "qwen2.5-coder:7b";

/// Classified user intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    CreateGoal,
    AddFeedback,
    Question,
}

/// How the intent was classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentMethod {
    Llm,
    Rule,
}

/// The outcome of intent classification.
#[derive(Debug, Clone, Serialize)]
pub struct IntentDecision {
    pub intent: Intent,
    pub method: IntentMethod,
    pub objective: Option<String>,
}

/// Result of one text-loop turn.
#[derive(Debug, Clone, Serialize)]
pub struct LoopOutcome {
    pub intent: Intent,
    pub intent_method: IntentMethod,
    pub response: String,
    pub goal_id: Option<String>,
}

/// Errors raised by the text loop.
#[derive(Debug, thiserror::Error)]
pub enum LoopError {
    #[error("store error: {0}")]
    Store(#[from] veyra_store::StoreError),
    #[error("AI error: {0}")]
    Ai(#[from] veyra_ai::AIError),
    #[error("tool error: {0}")]
    Tool(#[from] veyra_tools::ToolError),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("goal not found: {0}")]
    GoalNotFound(String),
}

/// Classifies user input into an [Intent].
pub struct IntentEngine {
    router: Arc<AIRouter>,
}

impl IntentEngine {
    pub fn new(router: Arc<AIRouter>) -> Self {
        Self { router }
    }

    /// Classify with the LLM first; fall back to deterministic rules.
    pub async fn classify(&self, text: &str) -> IntentDecision {
        match self.classify_llm(text).await {
            Some(decision) => decision,
            None => Self::classify_rule(text),
        }
    }

    async fn classify_llm(&self, text: &str) -> Option<IntentDecision> {
        let prompt = format!(
            "あなたはVeyraのインテント分類器です。ユーザー入力を次のどれかに分類してください: \
             create_goal(新しい目標・タスクを作る)、add_feedback(既存の結果への修正指摘)、\
             question(質問・相談)。\n\n\
             ユーザー入力: {text}\n\n\
             create_goalの場合は objective に目標文を入れてください。\n\
             JSONのみを返却してください。例: {{\"intent\": \"question\"}}"
        );
        let request = AIRequest {
            task_type: "intent".into(),
            prompt,
            context: json!({}),
            max_tokens: Some(120),
            temperature: Some(0.2),
        };
        let response = complete_with_fallback(
            &self.router,
            request,
            Some(INTENT_MODEL),
            &Capabilities::default(),
        )
        .await
        .ok()?;
        let value = extract_json(&response.text)?;
        let intent = match value["intent"].as_str()? {
            "create_goal" => Intent::CreateGoal,
            "add_feedback" => Intent::AddFeedback,
            "question" => Intent::Question,
            _ => return None,
        };
        Some(IntentDecision {
            intent,
            method: IntentMethod::Llm,
            objective: value["objective"].as_str().map(str::to_string),
        })
    }

    /// Deterministic keyword fallback (used when the LLM is unavailable).
    pub fn classify_rule(text: &str) -> IntentDecision {
        let goal_markers = [
            "作って", "作成", "作りたい", "完成させ", "実装", "目標", "追加し", "作る",
        ];
        let feedback_markers = [
            "違う", "ちがう", "修正", "直し", "だめ", "ダメ", "やめ", "変更", "もっと",
            "ここ", "おかしい",
        ];
        let intent = if goal_markers.iter().any(|m| text.contains(m)) {
            Intent::CreateGoal
        } else if feedback_markers.iter().any(|m| text.contains(m)) {
            Intent::AddFeedback
        } else {
            Intent::Question
        };
        IntentDecision { intent, method: IntentMethod::Rule, objective: None }
    }
}

/// One turn of the text loop, wired to the SQLite event log.
pub struct TextLoop {
    router: Arc<AIRouter>,
    engine: IntentEngine,
    store: Arc<Mutex<Store>>,
}

impl TextLoop {
    pub fn new(router: Arc<AIRouter>, store: Arc<Mutex<Store>>) -> Self {
        let engine = IntentEngine::new(router.clone());
        Self { router, engine, store }
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    fn append(
        &self,
        project_id: &str,
        event_type: EventType,
        payload: serde_json::Value,
    ) -> Result<(), LoopError> {
        self.lock()
            .append_event(&Event::now(project_id, event_type, payload))?;
        Ok(())
    }

    /// Compact project context for AI prompts (structured state, never
    /// the full event history — Plan.txt section 18/19).
    fn snapshot_context(&self, project_id: &str) -> String {
        let events = match self.lock().events_for_project(project_id) {
            Ok(events) => events,
            Err(_) => return "{}".to_string(),
        };
        let snapshot = Projector::project(&events);
        json!({
            "goals": snapshot.goals.iter().map(|g| json!({
                "objective": g.objective,
                "status": g.status,
                "requirements": g.requirements.len(),
                "acceptance_criteria": g.acceptance_criteria.len(),
            })).collect::<Vec<_>>(),
            "decisions": snapshot.decisions.iter().map(|d| d.decision.clone()).collect::<Vec<_>>(),
            "recent_feedback": snapshot.user_feedback.iter().rev().take(3).collect::<Vec<_>>(),
        })
        .to_string()
    }

    fn create_goal(&self, project_id: &str, objective: &str) -> Result<Goal, LoopError> {
        let now = chrono::Utc::now();
        let goal = Goal {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_string(),
            objective: objective.to_string(),
            requirements: vec![],
            constraints: vec![],
            preferences: vec![],
            acceptance_criteria: vec![],
            status: GoalStatus::Active,
            created_at: now,
            updated_at: now,
        };
        self.append(project_id, EventType::GoalCreated, json!({ "goal": goal }))?;
        Ok(goal)
    }

    fn record_provider_run(
        &self,
        project_id: &str,
        task_type: &str,
        response: &veyra_ai::AIResponse,
    ) -> Result<(), LoopError> {
        let now = chrono::Utc::now();
        self.lock().record_provider_run(&ProviderRunRecord {
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

    fn preferred_model(task_type: &str) -> Option<&'static str> {
        match task_type {
            "intent" => Some(INTENT_MODEL),
            "coding" => Some(CODING_MODEL),
            _ => None,
        }
    }

    /// Run one full turn of the text loop.
    pub async fn handle_input(
        &self,
        project_id: &str,
        text: &str,
    ) -> Result<LoopOutcome, LoopError> {
        // 1. Record the raw input.
        self.append(
            project_id,
            EventType::UserInput,
            json!({ "content": text, "source": "text" }),
        )?;

        // 2. Classify intent (LLM first, rule fallback).
        let decision = self.engine.classify(text).await;
        self.append(
            project_id,
            EventType::IntentDetected,
            json!({ "intent": decision.intent, "method": decision.method }),
        )?;

        // 3. Resolve the intent against the goal engine.
        let mut goal_id: Option<String> = None;
        let (task_type, prompt) = match decision.intent {
            Intent::CreateGoal => {
                let objective = decision
                    .objective
                    .clone()
                    .unwrap_or_else(|| text.to_string());
                let goal = self.create_goal(project_id, &objective)?;
                goal_id = Some(goal.id.clone());
                (
                    "planning",
                    format!(
                        "新しい目標「{objective}」が登録されました。\
                         最初に取り組むべき3ステップを簡潔に日本語で提案してください。"
                    ),
                )
            }
            Intent::AddFeedback => {
                self.append(project_id, EventType::UserFeedback, json!({ "content": text }))?;
                let context = self.snapshot_context(project_id);
                (
                    "review",
                    format!(
                        "ユーザーからのフィードバック: 「{text}」\n\n\
                         現在のプロジェクト状態:\n{context}\n\n\
                         この指摘にどう対応すべきか、次のアクションを簡潔に日本語で提案してください。"
                    ),
                )
            }
            Intent::Question => {
                let context = self.snapshot_context(project_id);
                (
                    "answer",
                    format!(
                        "質問: {text}\n\n現在のプロジェクト状態:\n{context}\n\n\
                         簡潔に日本語で回答してください。"
                    ),
                )
            }
        };

        // 4. Execute with the AI router (Generate ≠ Done: the response is
        //    recorded and will be verified by the Verification Engine in M4).
        let request = AIRequest {
            task_type: task_type.into(),
            prompt,
            context: json!({ "project_id": project_id }),
            max_tokens: Some(500),
            temperature: Some(0.3),
        };
        let ai_result = complete_with_fallback(
            &self.router,
            request,
            Self::preferred_model(task_type),
            &Capabilities::default(),
        )
        .await;

        // 5. Persist the AI run and the response.
        let response = match ai_result {
            Ok(resp) => {
                self.record_provider_run(project_id, task_type, &resp)?;
                self.append(
                    project_id,
                    EventType::AssistantResponse,
                    json!({ "content": resp.text }),
                )?;
                resp.text
            }
            Err(e) => format!("(AIの実行に失敗しました: {e})"),
        };

        Ok(LoopOutcome {
            intent: decision.intent,
            intent_method: decision.method,
            response,
            goal_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veyra_ai::MockProvider;
    use veyra_store::Projector;

    fn mock_router(reply: &str) -> Arc<AIRouter> {
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new(
            INTENT_MODEL,
            reply,
        )));
        Arc::new(router)
    }

    fn make_loop() -> (Arc<Mutex<Store>>, String) {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let project = store.lock().unwrap().create_project("proj", None).unwrap();
        (store.clone(), project.id)
    }

    #[test]
    fn rule_fallback_classifies_japanese_inputs() {
        let d = IntentEngine::classify_rule("ログイン機能を作って");
        assert_eq!(d.intent, Intent::CreateGoal);
        assert_eq!(d.method, IntentMethod::Rule);

        let d = IntentEngine::classify_rule("いや、ここ違うよ");
        assert_eq!(d.intent, Intent::AddFeedback);

        let d = IntentEngine::classify_rule("この設計の利点は何ですか?");
        assert_eq!(d.intent, Intent::Question);
    }

    #[tokio::test]
    async fn loop_runs_end_to_end_with_llm_intent() {
        let router = mock_router("{\"intent\": \"create_goal\", \"objective\": \"テスト目標\"}");
        let (store, project_id) = make_loop();
        let text_loop = TextLoop::new(router, store.clone());

        let outcome = text_loop.handle_input(&project_id, "テスト目標を作って").await.unwrap();

        assert_eq!(outcome.intent, Intent::CreateGoal);
        assert_eq!(outcome.intent_method, IntentMethod::Llm);
        assert!(outcome.goal_id.is_some());
        assert!(!outcome.response.is_empty());

        let events = store.lock().unwrap().events_for_project(&project_id).unwrap();
        let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
        assert!(types.contains(&EventType::UserInput));
        assert!(types.contains(&EventType::IntentDetected));
        assert!(types.contains(&EventType::GoalCreated));
        assert!(types.contains(&EventType::AssistantResponse));

        // The AI execution was persisted (Plan.txt section 40).
        let runs = store.lock().unwrap().provider_runs_for_project(&project_id).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].task_type.as_deref(), Some("planning"));

        // The projection sees the new goal.
        let snapshot = Projector::project(&events);
        assert_eq!(snapshot.goals.len(), 1);
        assert_eq!(snapshot.goals[0].objective, "テスト目標");
        assert_eq!(snapshot.messages.len(), 2); // user + assistant
    }

    #[tokio::test]
    async fn loop_falls_back_to_rules_when_llm_fails() {
        // Empty router: every AI call fails.
        let router = Arc::new(AIRouter::new());
        let (store, project_id) = make_loop();
        let text_loop = TextLoop::new(router, store.clone());

        let outcome = text_loop.handle_input(&project_id, "ログイン画面を作って").await.unwrap();

        assert_eq!(outcome.intent, Intent::CreateGoal);
        assert_eq!(outcome.intent_method, IntentMethod::Rule);
        assert!(outcome.goal_id.is_some());
        assert!(outcome.response.contains("失敗"));
    }
}


