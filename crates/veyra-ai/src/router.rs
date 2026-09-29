//! The AI Router chooses the best provider for a task (Plan.txt section 21).
//!
//! Inputs: task type, required capabilities, privacy requirement, available
//! providers. Output: selected provider (with fallback on failure).

use std::sync::Arc;

use crate::{AIError, AIProvider, AIRequest, AIResponse, Capabilities};

/// Check that `actual` capabilities cover the `required` ones.
pub fn satisfies(actual: Capabilities, required: &Capabilities) -> bool {
    (!required.streaming || actual.streaming)
        && (!required.tool_use || actual.tool_use)
        && (!required.vision || actual.vision)
        && (!required.local || actual.local)
}

/// The AI Router: selects providers and applies fallback.
pub struct AIRouter {
    providers: Vec<Arc<dyn AIProvider>>,
}

impl AIRouter {
    pub fn new() -> Self {
        Self { providers: Vec::new() }
    }

    /// Register a provider. Registration order is the fallback order.
    pub fn register(&mut self, provider: Arc<dyn AIProvider>) {
        self.providers.push(provider);
    }

    pub fn providers(&self) -> &[Arc<dyn AIProvider>] {
        &self.providers
    }

    /// Select a provider by explicit preference (id), falling back to the
    /// first registered provider that satisfies the required capabilities.
    /// Select a provider by explicit preference (id or model name), falling back to the
    /// first registered provider that satisfies the required capabilities.
    /// If an explicit preference is requested, it matches exact ID or prefix-stripped ID
    /// (e.g. "ollama-qwen2.5-coder:7b" matches "qwen2.5-coder:7b").
    pub fn select(
        &self,
        task_type: &str,
        preferred: Option<&str>,
        required: &Capabilities,
    ) -> Result<Arc<dyn AIProvider>, AIError> {
        if let Some(id) = preferred {
            if let Some(p) = self.providers.iter().find(|p| {
                let pid = &p.id().0;
                pid == id
                    || pid == &format!("ollama-{id}")
                    || pid.strip_prefix("ollama-") == Some(id)
                    || pid.ends_with(&format!("-{id}"))
            }) {
                return Ok(p.clone());
            }
        } else {
            // Smart Tiering: infer optimal model by task type (Plan §20-21 Multi-model Hybrid)
            let inferred = match task_type {
                "coding" | "agent_plan" | "verification" => Some("qwen2.5-coder:7b"),
                "intent" | "cleanup" | "summary" | "assistant_plan" => Some("qwen2.5:1.5b"),
                _ => None,
            };
            if let Some(id) = inferred {
                if let Some(p) = self.providers.iter().find(|p| {
                    let pid = &p.id().0;
                    pid == id
                        || pid == &format!("ollama-{id}")
                        || pid.strip_prefix("ollama-") == Some(id)
                        || pid.ends_with(&format!("-{id}"))
                }) {
                    return Ok(p.clone());
                }
            }
        }
        self.providers
            .iter()
            .find(|p| satisfies(p.capabilities(), required))
            .cloned()
            .ok_or_else(|| AIError::NoProvider(task_type.to_string()))
    }
}

impl Default for AIRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Route a request: try the selected provider, then fall back through the
/// remaining registered providers. Implemented as a free function so the
/// router itself stays sync.
pub async fn complete_with_fallback(
    router: &AIRouter,
    request: AIRequest,
    preferred: Option<&str>,
    required: &Capabilities,
) -> Result<AIResponse, AIError> {
    let selected = router.select(&request.task_type, preferred, required)?;
    let mut last_err = None;
    for provider in std::iter::once(selected).chain(
        router
            .providers
            .iter()
            .filter(|p| satisfies(p.capabilities(), required))
            .cloned(),
    ) {
        match provider.complete(request.clone()).await {
            Ok(resp) => return Ok(resp),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| AIError::NoProvider(request.task_type.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockProvider, ProviderId};
    use serde_json::json;

    fn tool_provider(id: &str) -> MockProvider {
        MockProvider {
            id: ProviderId(id.into()),
            caps: Capabilities { streaming: true, tool_use: true, vision: false, local: false },
            reply: format!("{id} done"),
        }
    }

    fn req() -> AIRequest {
        AIRequest {
            task_type: "coding".into(),
            prompt: "refactor".into(),
            context: json!({}),
            max_tokens: None,
            temperature: None,
        }
    }

    #[tokio::test]
    async fn router_prefers_explicit_provider() {
        let mut router = AIRouter::new();
        router.register(Arc::new(tool_provider("claude")));
        router.register(Arc::new(tool_provider("codex")));

        let resp = complete_with_fallback(&router, req(), Some("codex"), &Capabilities::default())
            .await
            .unwrap();
        assert_eq!(resp.provider, "codex");
        assert!(resp.text.contains("codex done"));
    }

    #[tokio::test]
    async fn router_falls_back_on_capability_match() {
        let mut router = AIRouter::new();
        router.register(Arc::new(tool_provider("claude")));
        router.register(Arc::new(MockProvider::new("local", "local done")));

        // Needs tool_use: only "claude" qualifies.
        let resp = complete_with_fallback(
            &router,
            req(),
            None,
            &Capabilities { tool_use: true, ..Default::default() },
        )
        .await
        .unwrap();
        assert_eq!(resp.provider, "claude");
    }

    #[tokio::test]
    async fn router_resolves_preferred_model_with_provider_prefix() {
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new("ollama-qwen2.5:1.5b", "1.5b reply")));
        router.register(Arc::new(MockProvider::new("ollama-qwen2.5-coder:7b", "coder reply")));

        // Calling with bare model name "qwen2.5-coder:7b" must resolve to "ollama-qwen2.5-coder:7b"
        let resp = complete_with_fallback(&router, req(), Some("qwen2.5-coder:7b"), &Capabilities::default())
            .await
            .unwrap();
        assert_eq!(resp.provider, "ollama-qwen2.5-coder:7b");
        assert!(resp.text.contains("coder reply"));
    }

    #[tokio::test]
    async fn router_falls_back_when_preferred_model_absent() {
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new("ollama-qwen2.5:1.5b", "1.5b reply")));

        // When preferred model is absent, it falls back to first provider satisfying capabilities
        let resp = complete_with_fallback(&router, req(), Some("nonexistent-model"), &Capabilities::default())
            .await
            .unwrap();
        assert_eq!(resp.provider, "ollama-qwen2.5:1.5b");
    }

    #[tokio::test]
    async fn router_smart_tiering_selects_coder_for_coding_and_light_for_intent() {
        let mut router = AIRouter::new();
        router.register(Arc::new(MockProvider::new("ollama-qwen2.5:1.5b", "light reply")));
        router.register(Arc::new(MockProvider::new("ollama-qwen2.5-coder:7b", "coder reply")));

        // Task "coding" without explicit preferred must pick coder:7b
        let coding_req = AIRequest {
            task_type: "coding".into(),
            prompt: "impl function".into(),
            context: json!({}),
            max_tokens: None,
            temperature: None,
        };
        let resp_coding = complete_with_fallback(&router, coding_req, None, &Capabilities::default())
            .await
            .unwrap();
        assert_eq!(resp_coding.provider, "ollama-qwen2.5-coder:7b");

        // Task "intent" without explicit preferred must pick 1.5b
        let intent_req = AIRequest {
            task_type: "intent".into(),
            prompt: "classify".into(),
            context: json!({}),
            max_tokens: None,
            temperature: None,
        };
        let resp_intent = complete_with_fallback(&router, intent_req, None, &Capabilities::default())
            .await
            .unwrap();
        assert_eq!(resp_intent.provider, "ollama-qwen2.5:1.5b");
    }
}
