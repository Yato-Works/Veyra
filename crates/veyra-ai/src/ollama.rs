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

/// Validates that an endpoint URL is strictly a loopback address (127.0.0.1, localhost, or [::1])
/// to prevent SSRF attacks against internal or external networks.
pub fn is_loopback_url(url_str: &str) -> bool {
    if let Ok(parsed) = reqwest::Url::parse(url_str) {
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return false;
        }
        match parsed.host_str() {
            Some("127.0.0.1") | Some("localhost") | Some("[::1]") => true,
            _ => false,
        }
    } else {
        false
    }
}

/// Cheap liveness probe for the local Ollama daemon (used by the UI status
/// indicator). Non-loopback URLs are redirected to the safe default endpoint.
/// Returns true when `GET /api/tags` answers with a success status in 2s.
pub async fn ollama_alive(base_url: &str) -> bool {
    let trimmed = base_url.trim_end_matches('/');
    let safe_base = if is_loopback_url(trimmed) {
        trimmed
    } else {
        DEFAULT_BASE_URL
    };
    let url = format!("{safe_base}/api/tags");
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(2)).build() {
        Ok(c) => c,
        Err(_) => return false,
    };
    matches!(client.get(&url).send().await, Ok(resp) if resp.status().is_success())
}

impl OllamaProvider {
    /// Provider for `model` on the default local Ollama endpoint.
    pub fn new(model: impl Into<String>) -> Self {
        Self::with_base_url(model, DEFAULT_BASE_URL)
    }

    /// Provider for `model` on a custom Ollama endpoint.
    ///
    /// The `base_url` MUST be a loopback address (127.0.0.1, localhost, or [::1]).
    /// Non-loopback URLs are safely replaced with the default to prevent
    /// SSRF attacks against internal services.
    pub fn with_base_url(model: impl Into<String>, base_url: &str) -> Self {
        let model = model.into();
        let trimmed = base_url.trim_end_matches('/');
        let effective_url = if is_loopback_url(trimmed) {
            trimmed.to_string()
        } else {
            DEFAULT_BASE_URL.to_string()
        };
        Self {
            id: ProviderId(format!("ollama-{model}")),
            model,
            base_url: effective_url,
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

    #[test]
    fn loopback_url_validation_blocks_spoofed_hosts() {
        assert!(is_loopback_url("http://127.0.0.1:11434"));
        assert!(is_loopback_url("http://localhost:11434"));
        assert!(is_loopback_url("http://[::1]:11434"));
        assert!(is_loopback_url("http://127.0.0.1"));
        assert!(is_loopback_url("http://localhost"));

        // Spoofed hosts that attempt to bypass prefix matching
        assert!(!is_loopback_url("http://127.0.0.1.attacker.com"));
        assert!(!is_loopback_url("http://localhost.attacker.com"));
        assert!(!is_loopback_url("http://127.0.0.1:80@attacker.com"));
        assert!(!is_loopback_url("http://192.168.1.1:11434"));
        assert!(!is_loopback_url("file:///etc/passwd"));
        assert!(!is_loopback_url("not-a-url"));

        // with_base_url defaults to safe endpoint when spoofed
        let safe = OllamaProvider::with_base_url("test", "http://127.0.0.1.attacker.com");
        assert_eq!(safe.base_url, DEFAULT_BASE_URL);
    }

    #[tokio::test]
    async fn live_ollama_responds() {
        let p = OllamaProvider::new("qwen2.5:1.5b");
        let req = AIRequest {
            task_type: "intent".into(),
            prompt: "こんにちは！1+1の答えを数字だけで教えてください。".into(),
            context: serde_json::json!({}),
            max_tokens: Some(50),
            temperature: Some(0.1),
        };
        let res = p.complete(req).await;
        match res {
            Ok(resp) => {
                println!("Live Ollama output: {}", resp.text);
                assert!(!resp.text.trim().is_empty());
            }
            Err(e) => {
                eprintln!("Ollama live test skipped (server not available or model not ready): {e}");
            }
        }
    }
}
