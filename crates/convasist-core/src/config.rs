//! Application configuration (persisted as JSON in the app-data dir).
//!
//! API keys are NOT part of this struct — they live in the OS credential
//! vault (§4.6), keyed by provider id.

use serde::{Deserialize, Serialize};

use crate::asr::AsrEngineId;
use crate::llm::{provider_registry, ModelSelection, DEFAULT_PROVIDER};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Transcription engine (local whisper default; cloud opt-in).
    pub asr_engine: AsrEngineId,
    /// Whisper model name for the local engine (e.g. "base.en").
    pub whisper_model: String,
    /// Quality slot: on-demand assists (§4.5 O4).
    pub llm_quality: ModelSelection,
    /// Fast slot: proactive/cheap paths. `None` mirrors the quality slot
    /// (the "same as quality" toggle, §4.6).
    pub llm_fast: Option<ModelSelection>,
    /// User acknowledged the recording-consent notice (§7.1). The app will
    /// not start a capture session while this is false.
    pub consent_acknowledged: bool,
    /// Preferred microphone device name (`None` = system default; A3).
    pub input_device: Option<String>,
    /// Preferred loopback source — an OUTPUT device whose playback is
    /// captured (`None` = default output; A2/A3).
    pub loopback_device: Option<String>,
    /// Commitment & entity tracker (§6.3) — runs fast-slot LLM passes over
    /// finalized speech during a session. Requires a stored API key.
    pub tracker_enabled: bool,
    /// Neural VAD (Silero): filter background noise so only real speech is
    /// transcribed. Falls back to the energy gate if the model isn't ready.
    pub vad_neural: bool,
    /// Noise-filter strength in [0, 1] (higher = filter more aggressively).
    pub vad_sensitivity: f32,
}

impl Default for AppConfig {
    fn default() -> Self {
        let registry = provider_registry();
        let default_provider = registry
            .iter()
            .find(|p| p.id == DEFAULT_PROVIDER)
            .expect("default provider must be in registry");
        Self {
            asr_engine: AsrEngineId::WhisperLocal,
            // Low-latency default: quantized tiny is the fastest/smallest
            // whisper checkpoint. Users can trade up for accuracy in Settings.
            whisper_model: "tiny.en-q5_1".to_string(),
            llm_quality: ModelSelection {
                provider: DEFAULT_PROVIDER,
                model: default_provider.default_quality_model.to_string(),
            },
            llm_fast: Some(ModelSelection {
                provider: DEFAULT_PROVIDER,
                model: default_provider.default_fast_model.to_string(),
            }),
            consent_acknowledged: false,
            input_device: None,
            loopback_device: None,
            tracker_enabled: true,
            vad_neural: true,
            vad_sensitivity: 0.5,
        }
    }
}

impl AppConfig {
    /// The fast slot resolves to the quality slot when unset.
    pub fn fast_selection(&self) -> &ModelSelection {
        self.llm_fast.as_ref().unwrap_or(&self.llm_quality)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::ProviderId;

    #[test]
    fn default_config_is_local_asr_and_claude() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.asr_engine, AsrEngineId::WhisperLocal);
        assert_eq!(cfg.llm_quality.provider, ProviderId::Anthropic);
        assert_eq!(cfg.llm_quality.model, "claude-sonnet-5");
        assert_eq!(cfg.fast_selection().model, "claude-haiku-4-5");
        assert!(!cfg.consent_acknowledged, "consent must be opt-in");
    }

    #[test]
    fn fast_slot_falls_back_to_quality() {
        let cfg = AppConfig {
            llm_fast: None,
            ..Default::default()
        };
        assert_eq!(cfg.fast_selection(), &cfg.llm_quality);
    }

    #[test]
    fn config_round_trips_and_tolerates_missing_fields() {
        let cfg = AppConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);

        // Forward-compat: an older config file missing new fields still loads.
        let sparse: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(sparse, AppConfig::default());
    }
}
