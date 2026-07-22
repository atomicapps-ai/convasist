//! AI Orchestration Layer — LLM provider abstraction (design §4.6).
//!
//! Owner decision 2026-07-09: the UI supports configuring any AI with a
//! default of Claude; a dropdown selects the provider and the model. This
//! module is the single source of truth for the launch provider registry —
//! the settings UI renders exactly what `provider_registry()` returns.
//!
//! Adding a provider = one `LlmProvider` implementation file in `src-tauri`
//! plus one `ProviderInfo` entry here. The orchestrator is provider-blind.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::CoreError;

/// Stable identifiers for the launch providers (§4.6 registry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Anthropic,
    Openai,
    Google,
    Xai,
    Deepseek,
    /// Local OpenAI-compatible endpoint (Ollama) — stretch.
    OllamaLocal,
}

/// Registry metadata driving the settings dropdowns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: ProviderId,
    /// Display name for the provider dropdown.
    pub name: &'static str,
    /// Curated default for the quality slot (on-demand assists).
    pub default_quality_model: &'static str,
    /// Curated default for the fast slot (proactive/cheap paths).
    pub default_fast_model: &'static str,
    /// Whether this provider requires an API key (Ollama does not).
    pub requires_api_key: bool,
    /// Whether inference happens on-device (privacy indicator in settings).
    pub is_local: bool,
}

/// The launch registry — order here is dropdown order; the first entry is
/// the application default.
pub fn provider_registry() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: ProviderId::Anthropic,
            name: "Anthropic Claude",
            default_quality_model: "claude-sonnet-5",
            default_fast_model: "claude-haiku-4-5",
            requires_api_key: true,
            is_local: false,
        },
        ProviderInfo {
            id: ProviderId::Openai,
            name: "OpenAI",
            default_quality_model: "gpt-5.2",
            default_fast_model: "gpt-5-mini",
            requires_api_key: true,
            is_local: false,
        },
        ProviderInfo {
            id: ProviderId::Google,
            name: "Google Gemini",
            default_quality_model: "gemini-3-pro",
            default_fast_model: "gemini-3-flash",
            requires_api_key: true,
            is_local: false,
        },
        ProviderInfo {
            id: ProviderId::Xai,
            name: "xAI Grok",
            default_quality_model: "grok-4",
            default_fast_model: "grok-4-fast",
            requires_api_key: true,
            is_local: false,
        },
        ProviderInfo {
            id: ProviderId::Deepseek,
            name: "DeepSeek",
            default_quality_model: "deepseek-chat",
            default_fast_model: "deepseek-chat",
            requires_api_key: true,
            is_local: false,
        },
        ProviderInfo {
            id: ProviderId::OllamaLocal,
            name: "Ollama (local)",
            default_quality_model: "llama3.1:8b",
            default_fast_model: "llama3.1:8b",
            requires_api_key: false,
            is_local: true,
        },
    ]
}

/// The application default provider (first registry entry).
pub const DEFAULT_PROVIDER: ProviderId = ProviderId::Anthropic;

/// One provider+model pair as selected in a settings dropdown pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSelection {
    pub provider: ProviderId,
    pub model: String,
}

/// A chat-style request assembled by the context builder (§4.5 O1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
}

/// Incremental output from a streaming completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LlmChunk {
    /// A piece of assistant text.
    Token(String),
    /// Stream finished cleanly.
    Done,
}

/// A model as reported by a provider's live model-list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
}

/// A streaming LLM provider. Implementations live in `src-tauri` (one file
/// per provider) and normalize each provider's SSE schema into `LlmChunk`s.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    /// Stream a completion; `sink` receives chunks as they arrive.
    async fn stream_completion(
        &self,
        model: &str,
        request: LlmRequest,
        sink: Box<dyn FnMut(LlmChunk) + Send>,
    ) -> Result<(), CoreError>;

    /// Live model list where the provider offers one; falls back to the
    /// curated defaults in `provider_registry()` otherwise.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, CoreError>;

    /// One cheap round-trip validating the key; returns measured first-token
    /// latency in ms (the settings "Test" button, §4.6).
    async fn validate_key(&self) -> Result<u32, CoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_default_is_anthropic_and_first() {
        let registry = provider_registry();
        assert_eq!(registry[0].id, DEFAULT_PROVIDER);
        assert_eq!(registry[0].id, ProviderId::Anthropic);
    }

    #[test]
    fn registry_has_five_cloud_providers_plus_local() {
        let registry = provider_registry();
        assert_eq!(registry.iter().filter(|p| !p.is_local).count(), 5);
        assert_eq!(registry.iter().filter(|p| p.is_local).count(), 1);
    }

    #[test]
    fn only_local_providers_skip_api_keys() {
        for p in provider_registry() {
            assert_eq!(p.requires_api_key, !p.is_local, "provider {:?}", p.id);
        }
    }

    #[test]
    fn provider_ids_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&ProviderId::Anthropic).unwrap(),
            "\"anthropic\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderId::OllamaLocal).unwrap(),
            "\"ollama_local\""
        );
    }
}
