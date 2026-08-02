//! Usage metering — the pure ledger behind the Settings → Usage panel.
//!
//! conva is bring-your-own-key on the desktop, so metering here is about
//! **visibility**: the owner sees exactly what their keys are being spent on
//! (LLM tokens per provider, plus Tavily web searches — Tavily bills per
//! *search*, not per token). The hosted future turns the same counts into
//! billable credits (roadmap F8b, `docs/platform/04-billing-credits.md`); this
//! module stays pure so both surfaces share one accounting model.
//!
//! The shell (`src-tauri/src/metering.rs`) owns persistence and calls
//! [`UsageLedger::record_llm`] / [`UsageLedger::record_tavily_search`] at each
//! metered call site. Everything here is fs/OS-free and unit-tested.

use serde::{Deserialize, Serialize};

use crate::llm::{ProviderId, TokenUsage};

/// Running usage for a single LLM provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider: ProviderId,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Number of completions attributed to this provider.
    pub requests: u64,
}

impl ProviderUsage {
    fn new(provider: ProviderId) -> Self {
        Self {
            provider,
            input_tokens: 0,
            output_tokens: 0,
            requests: 0,
        }
    }
}

/// The persisted usage ledger. One per machine; the shell mirrors it to
/// `<app-data>/usage.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageLedger {
    /// Per-provider LLM token totals.
    #[serde(default)]
    pub providers: Vec<ProviderUsage>,
    /// Tavily web searches (each Tavily query is one billed search).
    #[serde(default)]
    pub tavily_searches: u64,
    /// Text-to-speech characters synthesized (Deepgram Aura bills per character).
    #[serde(default)]
    pub tts_characters: u64,
    /// When the current accounting window opened (first record, or last reset).
    /// `0` means "not started yet".
    #[serde(default)]
    pub since_unix_ms: u64,
    /// Last time any counter moved.
    #[serde(default)]
    pub updated_at_unix_ms: u64,
}

impl UsageLedger {
    fn start_window(&mut self, now_unix_ms: u64) {
        if self.since_unix_ms == 0 {
            self.since_unix_ms = now_unix_ms;
        }
        self.updated_at_unix_ms = now_unix_ms;
    }

    /// Attribute one completion's token usage to `provider`. A zero-token
    /// usage (provider reported nothing) still counts as one request, so the
    /// request tally stays honest even when token counts are unavailable.
    pub fn record_llm(&mut self, provider: ProviderId, usage: TokenUsage, now_unix_ms: u64) {
        self.start_window(now_unix_ms);
        let entry = match self.providers.iter_mut().find(|p| p.provider == provider) {
            Some(e) => e,
            None => {
                self.providers.push(ProviderUsage::new(provider));
                self.providers
                    .last_mut()
                    .expect("just pushed a provider entry")
            }
        };
        entry.input_tokens = entry.input_tokens.saturating_add(usage.input_tokens);
        entry.output_tokens = entry.output_tokens.saturating_add(usage.output_tokens);
        entry.requests = entry.requests.saturating_add(1);
    }

    /// Count `count` Tavily searches (one per bounded research query issued).
    pub fn record_tavily_search(&mut self, count: u64, now_unix_ms: u64) {
        if count == 0 {
            return;
        }
        self.start_window(now_unix_ms);
        self.tavily_searches = self.tavily_searches.saturating_add(count);
    }

    /// Count `chars` synthesized by text-to-speech (Aura bills per character).
    pub fn record_tts_characters(&mut self, chars: u64, now_unix_ms: u64) {
        if chars == 0 {
            return;
        }
        self.start_window(now_unix_ms);
        self.tts_characters = self.tts_characters.saturating_add(chars);
    }

    /// Clear all counters, reopening the window at `now`.
    pub fn reset(&mut self, now_unix_ms: u64) {
        *self = UsageLedger {
            since_unix_ms: now_unix_ms,
            updated_at_unix_ms: now_unix_ms,
            ..Default::default()
        };
    }

    /// A UI-ready snapshot with cross-provider totals precomputed.
    pub fn summary(&self) -> UsageSummary {
        let total_input_tokens = self.providers.iter().map(|p| p.input_tokens).sum();
        let total_output_tokens = self.providers.iter().map(|p| p.output_tokens).sum();
        let total_requests = self.providers.iter().map(|p| p.requests).sum();
        UsageSummary {
            providers: self.providers.clone(),
            total_input_tokens,
            total_output_tokens,
            total_requests,
            tavily_searches: self.tavily_searches,
            tts_characters: self.tts_characters,
            since_unix_ms: self.since_unix_ms,
            updated_at_unix_ms: self.updated_at_unix_ms,
        }
    }
}

/// What the Settings → Usage panel renders — the ledger plus running totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageSummary {
    pub providers: Vec<ProviderUsage>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_requests: u64,
    pub tavily_searches: u64,
    pub tts_characters: u64,
    pub since_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_record_opens_the_window() {
        let mut led = UsageLedger::default();
        assert_eq!(led.since_unix_ms, 0);
        led.record_llm(
            ProviderId::Anthropic,
            TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
            1_000,
        );
        assert_eq!(led.since_unix_ms, 1_000);
        assert_eq!(led.updated_at_unix_ms, 1_000);
    }

    #[test]
    fn llm_usage_accumulates_per_provider() {
        let mut led = UsageLedger::default();
        led.record_llm(
            ProviderId::Anthropic,
            TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
            1,
        );
        led.record_llm(
            ProviderId::Anthropic,
            TokenUsage {
                input_tokens: 3,
                output_tokens: 7,
            },
            2,
        );
        led.record_llm(
            ProviderId::Openai,
            TokenUsage {
                input_tokens: 100,
                output_tokens: 20,
            },
            3,
        );

        let sum = led.summary();
        assert_eq!(sum.providers.len(), 2);
        let anthropic = sum
            .providers
            .iter()
            .find(|p| p.provider == ProviderId::Anthropic)
            .unwrap();
        assert_eq!(anthropic.input_tokens, 13);
        assert_eq!(anthropic.output_tokens, 12);
        assert_eq!(anthropic.requests, 2);

        assert_eq!(sum.total_input_tokens, 113);
        assert_eq!(sum.total_output_tokens, 32);
        assert_eq!(sum.total_requests, 3);
    }

    #[test]
    fn zero_token_usage_still_counts_a_request() {
        let mut led = UsageLedger::default();
        led.record_llm(ProviderId::OllamaLocal, TokenUsage::default(), 1);
        let p = &led.summary().providers[0];
        assert_eq!(p.requests, 1);
        assert_eq!(p.input_tokens, 0);
    }

    #[test]
    fn tavily_searches_count_and_ignore_zero() {
        let mut led = UsageLedger::default();
        led.record_tavily_search(0, 1);
        assert_eq!(led.tavily_searches, 0);
        assert_eq!(
            led.since_unix_ms, 0,
            "a zero count must not open the window"
        );
        led.record_tavily_search(3, 5);
        led.record_tavily_search(2, 6);
        assert_eq!(led.tavily_searches, 5);
        assert_eq!(led.since_unix_ms, 5);
    }

    #[test]
    fn reset_clears_everything_and_reopens() {
        let mut led = UsageLedger::default();
        led.record_llm(
            ProviderId::Anthropic,
            TokenUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
            1,
        );
        led.record_tavily_search(4, 2);
        led.record_tts_characters(120, 3);
        assert_eq!(led.tts_characters, 120);
        led.reset(50);
        assert!(led.providers.is_empty());
        assert_eq!(led.tavily_searches, 0);
        assert_eq!(led.tts_characters, 0);
        assert_eq!(led.since_unix_ms, 50);
        assert_eq!(led.updated_at_unix_ms, 50);
    }
}
