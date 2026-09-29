//! Veyra AI Gateway: provider-independent access to intelligence engines
//! (Plan.txt sections 20-21).
//!
//! All models communicate through the [`AIProvider`] interface. The Runtime
//! never knows model-specific APIs: "LLM ≠ Runtime".
//!
//! ```text
//! Veyra Runtime → AI Gateway → GPT / Claude / Codex / Gemini / Ollama / ...
//! ```

mod ollama;
mod openai;
mod router;

pub use ollama::{ollama_alive, OllamaProvider};
pub use openai::OpenAICompatibleProvider;
pub use router::{complete_with_fallback, satisfies, AIRouter};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Identifier of a provider (e.g. "openai", "anthropic", "ollama-local").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(pub String);

/// What a provider can do. The AI Router uses this to select providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub streaming: bool,
    pub tool_use: bool,
    pub vision: bool,
    /// Runs fully locally (privacy-preserving).
    pub local: bool,
}

/// A task type hint for routing (e.g. "coding", "review", "transcription").
pub type TaskType = String;

/// A request sent to a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRequest {
    pub task_type: TaskType,
    pub prompt: String,
    /// Structured context package (Plan.txt section 18), never raw history.
    pub context: Value,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// A response from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponse {
    pub provider: String,
    pub model: String,
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Errors that can occur when talking to a provider.
#[derive(Debug, thiserror::Error)]
pub enum AIError {
    #[error("provider '{0}' failed: {1}")]
    ProviderFailed(String, String),
    #[error("no provider available for task '{0}'")]
    NoProvider(String),
}

/// The provider interface every AI engine implements (Plan.txt section 20).
#[async_trait]
pub trait AIProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    fn capabilities(&self) -> Capabilities;

    async fn complete(&self, request: AIRequest) -> Result<AIResponse, AIError>;
}

/// A deterministic mock provider for tests and offline development.
pub struct MockProvider {
    pub id: ProviderId,
    pub caps: Capabilities,
    pub reply: String,
}

impl MockProvider {
    pub fn new(id: &str, reply: impl Into<String>) -> Self {
        Self {
            id: ProviderId(id.to_string()),
            caps: Capabilities::default(),
            reply: reply.into(),
        }
    }
}

#[async_trait]
impl AIProvider for MockProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    async fn complete(&self, _request: AIRequest) -> Result<AIResponse, AIError> {
        Ok(AIResponse {
            provider: self.id.0.clone(),
            model: format!("{}-mock", self.id.0),
            // Return exactly the canned reply: a mock must never echo the
            // prompt, or prompt-embedded examples (e.g. `{"intent": ...}`)
            // would leak into responses and confuse JSON extraction.
            text: self.reply.clone(),
            input_tokens: None,
            output_tokens: None,
        })
    }
}

/// Extract the last JSON object from an LLM response.
///
/// Lessons learned from real-model usage (mirroring AIStoryActingEngine):
/// - `format`-style schema enforcement makes some models copy schema
///   examples into the output, so we instruct "JSON only" in the prompt
///   and extract structurally instead.
/// - Reasoning models may emit `<think>...</think>` blocks that can
///   contain draft JSON. We scan for *balanced* `{...}` spans (string-aware)
///   and prefer the last parseable object — the final answer.
///
/// Returns `None` when no parseable JSON object is present.
pub fn extract_json(text: &str) -> Option<serde_json::Value> {
    let mut last: Option<serde_json::Value> = None;
    let mut depth: usize = 0;
    let mut start: Option<usize> = None;
    let mut in_string = false;
    let mut escaped = false;

    for (i, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start {
                            if let Ok(v) = serde_json::from_str(&text[s..=i]) {
                                last = Some(v);
                            }
                        }
                        start = None;
                    }
                }
            }
            _ => {}
        }
    }

    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_from_plain_json() {
        let v = extract_json(r#"{"intent": "create_goal"}"#).unwrap();
        assert_eq!(v["intent"], "create_goal");
    }

    #[test]
    fn extract_json_ignores_surrounding_prose() {
        let text = "はい、分類しました。\n{\"intent\": \"question\"}\n以上です。";
        let v = extract_json(text).unwrap();
        assert_eq!(v["intent"], "question");
    }

    #[test]
    fn extract_json_tolerates_think_blocks() {
        let text = "<think>ユーザーはゴールを作りたいのだ。\n{\"draft\": 1}</think>\n{\"intent\": \"create_goal\"}";
        let v = extract_json(text).unwrap();
        assert_eq!(v["intent"], "create_goal");
    }

    #[test]
    fn extract_json_returns_none_without_json() {
        assert!(extract_json("JSONはありません").is_none());
        assert!(extract_json("{壊れた").is_none());
    }
}

