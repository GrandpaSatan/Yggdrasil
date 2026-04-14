use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Supported cloud LLM providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloudProvider {
    Gemini,
    Claude,
    Openai,
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gemini => write!(f, "gemini"),
            Self::Claude => write!(f, "claude"),
            Self::Openai => write!(f, "openai"),
        }
    }
}

/// A chat message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Request to a cloud LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
}

/// Response from a cloud LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub provider: CloudProvider,
    pub usage: TokenUsage,
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Model information advertised by a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: CloudProvider,
    pub context_window: u32,
}

/// Trait for cloud LLM adapters.
#[async_trait]
pub trait CloudAdapter: Send + Sync {
    async fn chat_completion(&self, req: ChatRequest) -> Result<ChatResponse, CloudError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, CloudError>;
    fn provider(&self) -> CloudProvider;
}

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error from {provider}: {message}")]
    Api {
        provider: CloudProvider,
        message: String,
        status: u16,
    },

    #[error("rate limited by {provider}, retry after {retry_after_secs}s")]
    RateLimited {
        provider: CloudProvider,
        retry_after_secs: u64,
    },

    #[error("missing API key for {0}")]
    MissingApiKey(CloudProvider),

    #[error("deserialization error: {0}")]
    Deserialize(String),
}
