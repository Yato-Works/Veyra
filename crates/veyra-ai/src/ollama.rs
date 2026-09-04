//! Ollama provider: a thin HTTP client to a local Ollama server
//! (http://localhost:11434).
//!
//! Mirrors the proven pattern from AIStoryActingEngine:
//! - The model runs in a separate process (Ollama daemon); this crate only
//!   consumes it over HTTP ("LLM ≠ Runtime").
//! - JSON output is requested in the prompt and extracted structurally —
//!   `format`-style schema enforcement is avoided.
//! - Requests are retried on transient failures.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{AIError, AIProvider, AIRequest, AIResponse, Capabilities, ProviderId};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_RETRIES: usize = 2;

/// A provider backed by a local Ollama model.
pub struct OllamaProvider {
    id: ProviderId,
    model: String,
    base_url: String,
    http: reqwest::Client,
}

impl OllamaProvider {
    /// Provider for `model` on the default local Ollama endpoint.
    pub fn new(model: impl Into<String>) -> Self {
        Self::with_base_url(model, DEFAULT_BASE_URL)
    }

    /// Provider for `model` on a custom Ollama endpoint.
    pub fn with_base_url(model: impl Into<String>, base_url: &str) -> Self {
        let model = model.into();
        Self {
            id: ProviderId(format!("ollama-{model}")),
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("failed to build reqwest client"),
        }
    }

    async fn post_with_retry(&self, payload: &Value) -> Result<Value, AIError> {
        let url = format!("{}/api/generate", self.base_url);
        let mut last_err: Option<AIError> = None;
        for _ in 0..=MAX_RETRIES {
            let result = self
                .http
                .post(&url)
                .json(payload)
                .send()
                .await;
            match result {
                Ok(resp) => match resp.error_for_status() {
                    Ok(ok) => match ok.json::<Value>().await {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            last_err = Some(AIError::ProviderFailed(
                                self.model.clone(),
                                format!("invalid response body: {e}"),
                            ))
                        }
                    },
                    Err(e) => last_err = Some(AIError::ProviderFailed(self.model.clone(), e.to_string())),
                },
                Err(e) => last_err = Some(AIError::ProviderFailed(self.model.clone(), e.to_string())),
            }
        }
        Err(last_err
            .unwrap_or_else(|| AIError::ProviderFailed(self.model.clone(), "unknown error".into())))
    }
}

#[async_trait]
impl AIProvider for OllamaProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            local: true,
        }
    }

    async fn complete(&self, request: AIRequest) -> Result<AIResponse, AIError> {
        let system = format!(
            "You are Veyra, an AI operating layer that turns human intent \
             into verified execution. Current project context:\n{}",
            serde_json::to_string_pretty(&request.context)
                .unwrap_or_else(|_| "{}".to_string())
        );

        let mut payload = json!({
            "model": self.model,
            "system": system,
            "prompt": request.prompt,
            "stream": false,
            "options": {
                "temperature": request.temperature.unwrap_or(0.3),
                "num_ctx": 8192,
            },
        });
        if let Some(max) = request.max_tokens {
            payload["options"]["num_predict"] = json!(max);
        }

        let resp = self.post_with_retry(&payload).await?;
        Ok(AIResponse {
            provider: self.id.0.clone(),
            model: self.model.clone(),
            text: resp["response"].as_str().unwrap_or_default().to_string(),
            input_tokens: resp["prompt_eval_count"].as_u64(),
            output_tokens: resp["eval_count"].as_u64(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_contains_model_name() {
        let p = OllamaProvider::new("qwen2.5:1.5b");
        assert_eq!(p.id().0, "ollama-qwen2.5:1.5b");
        assert!(p.capabilities().local);
    }

    #[tokio::test]
    #[ignore = "requires a running local Ollama server"]
    async fn live_ollama_responds() {
        let p = OllamaProvider::new("qwen2.5:1.5b");
        let req = AIRequest {
            task_type: "intent".into(),
            prompt: "コンニチハ".into(),
            context: serde_json::json!({}),
            max_tokens: Some(50),
            temperature: Some(0.2),
        };
        let resp = p.complete(req).await.expect("live ollama call");
        assert!(!resp.text.is_empty());
        println!("live response: {}", resp.text);
    }
}
