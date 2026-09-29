//! Generic OpenAI-compatible API Provider (OpenAI, DeepSeek, Groq, OpenRouter, vLLM, LM Studio).
//!
//! Enables connecting Veyra to external cloud LLM APIs or alternate local inference servers
//! conforming to the standard `/v1/chat/completions` REST format.

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;

use crate::{AIError, AIProvider, AIRequest, AIResponse, Capabilities, ProviderId};

/// Provider for OpenAI or any OpenAI-compatible API endpoint.
pub struct OpenAICompatibleProvider {
    id: ProviderId,
    model: String,
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl OpenAICompatibleProvider {
    /// Create a new OpenAI-compatible provider.
    ///
    /// * `model`: Target model name (e.g. "gpt-4o", "deepseek-coder", "llama-3.3-70b-versatile").
    /// * `base_url`: Base URL without trailing slash (e.g. "https://api.openai.com/v1", "http://localhost:1234/v1").
    /// * `api_key`: Optional API key (required for OpenAI/DeepSeek/Groq, optional for local tools).
    pub fn new(model: impl Into<String>, base_url: impl Into<String>, api_key: Option<String>) -> Self {
        let m = model.into();
        let b = base_url.into().trim_end_matches('/').to_string();
        let id_str = format!("openai-compat-{}", m);
        Self {
            id: ProviderId(id_str),
            model: m,
            base_url: b,
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl AIProvider for OpenAICompatibleProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            streaming: false,
            tool_use: true,
            vision: false,
            local: self.base_url.contains("localhost") || self.base_url.contains("127.0.0.1"),
        }
    }

    async fn complete(&self, request: AIRequest) -> Result<AIResponse, AIError> {
        let endpoint = format!("{}/chat/completions", self.base_url);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(ref key) = self.api_key {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", key.trim())) {
                headers.insert(AUTHORIZATION, val);
            }
        }

        let body = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": request.prompt
                }
            ],
            "temperature": request.temperature.unwrap_or(0.2),
        });

        let resp = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AIError::ProviderFailed(self.id.0.clone(), format!("HTTP error: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AIError::ProviderFailed(
                self.id.0.clone(),
                format!("API returned error status {status}: {text}"),
            ));
        }

        let val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AIError::ProviderFailed(self.id.0.clone(), format!("JSON decode error: {e}")))?;

        let text = val["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let input_tokens = val["usage"]["prompt_tokens"].as_u64();
        let output_tokens = val["usage"]["completion_tokens"].as_u64();

        Ok(AIResponse {
            provider: self.id.0.clone(),
            model: self.model.clone(),
            text,
            input_tokens,
            output_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_compat_provider_metadata() {
        let p = OpenAICompatibleProvider::new("deepseek-chat", "https://api.deepseek.com/v1", Some("sk-test".into()));
        assert_eq!(p.id().0, "openai-compat-deepseek-chat");
        assert!(!p.capabilities().local);

        let local_p = OpenAICompatibleProvider::new("local-model", "http://localhost:1234/v1", None);
        assert!(local_p.capabilities().local);
    }
}
