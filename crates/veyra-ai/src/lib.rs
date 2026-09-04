//! Veyra AI Gateway: provider-independent access to intelligence engines
//! (Plan.txt sections 20-21).
//!
//! All models communicate through the [`AIProvider`] interface. The Runtime
//! never knows model-specific APIs: "LLM ≠ Runtime".
//!
//! ```text
//! Veyra Runtime → AI Gateway → GPT / Claude / Codex / Gemini / Ollama / ...
//! ```

mod router;

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

    async fn complete(&self, request: AIRequest) -> Result<AIResponse, AIError> {
        Ok(AIResponse {
            provider: self.id.0.clone(),
            model: format!("{}-mock", self.id.0),
            text: format!("{}: {}", self.reply, request.prompt),
            input_tokens: None,
            output_tokens: None,
        })
    }
}
