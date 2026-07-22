//! Commitment & entity tracker (design §6.3): a fast-slot LLM pass over
//! newly finalized segments extracts names, amounts, dates, and
//! commitments ("I'll send the contract Friday") into a pinned side panel.
//!
//! This module owns the prompt and the (defensive) parsing of the model's
//! JSON; the shell owns batching, the LLM call, and dedupe/merge.

use serde::{Deserialize, Serialize};

use crate::asr::TranscriptSegment;
use crate::audio::StreamSide;
use crate::llm::LlmRequest;

pub const TRACKER_SYSTEM_PROMPT: &str = "You extract structured facts from \
live conversation transcripts. THEM lines are the other party, YOU lines \
are the user. Reply with ONLY a JSON object, no prose, no code fences, \
matching exactly: {\"entities\": [{\"label\": string, \"detail\": string}], \
\"commitments\": [{\"who\": \"you\"|\"them\", \"what\": string, \"due\": \
string}]}. entities: people, companies, dollar amounts, dates, product \
names — label is the thing, detail is one short clause of context. \
commitments: promises or action items someone took on; due is the stated \
deadline or \"\" if none. Extract only what is explicitly said. Empty \
arrays are fine.";

/// One extraction pass request over newly finalized segments.
pub fn build_tracker_request(segments: &[TranscriptSegment]) -> LlmRequest {
    let mut transcript = String::new();
    for segment in segments.iter().filter(|s| s.is_final) {
        let speaker = match segment.side {
            StreamSide::Inbound => "THEM",
            StreamSide::Outbound => "YOU",
        };
        transcript.push_str(&format!("{speaker}: {}\n", segment.text.trim()));
    }
    LlmRequest {
        system: TRACKER_SYSTEM_PROMPT.to_string(),
        user: transcript,
        max_tokens: 700,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TrackerExtraction {
    #[serde(default)]
    pub entities: Vec<TrackedEntity>,
    #[serde(default)]
    pub commitments: Vec<TrackedCommitment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackedEntity {
    pub label: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackedCommitment {
    /// "you" or "them".
    pub who: String,
    pub what: String,
    #[serde(default)]
    pub due: String,
}

/// Parse the model's reply. Tolerates code fences and surrounding prose by
/// slicing the outermost braces; returns `None` when nothing parses —
/// tracker updates are best-effort and silently skippable.
pub fn parse_tracker_reply(reply: &str) -> Option<TrackerExtraction> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&reply[start..=end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json() {
        let reply = r#"{"entities":[{"label":"Acme Corp","detail":"the customer"}],
            "commitments":[{"who":"you","what":"send the contract","due":"Friday"}]}"#;
        let parsed = parse_tracker_reply(reply).unwrap();
        assert_eq!(parsed.entities[0].label, "Acme Corp");
        assert_eq!(parsed.commitments[0].due, "Friday");
    }

    #[test]
    fn parses_fenced_json_with_prose() {
        let reply = "Here you go:\n```json\n{\"entities\":[],\"commitments\":[{\"who\":\"them\",\"what\":\"call back tomorrow\",\"due\":\"tomorrow\"}]}\n```";
        let parsed = parse_tracker_reply(reply).unwrap();
        assert_eq!(parsed.commitments.len(), 1);
    }

    #[test]
    fn missing_optional_fields_default() {
        let parsed = parse_tracker_reply(r#"{"entities":[{"label":"$4,500"}]}"#).unwrap();
        assert_eq!(parsed.entities[0].detail, "");
        assert!(parsed.commitments.is_empty());
    }

    #[test]
    fn garbage_returns_none() {
        assert!(parse_tracker_reply("I couldn't find anything.").is_none());
        assert!(parse_tracker_reply("").is_none());
        assert!(parse_tracker_reply("{not json}").is_none());
    }

    #[test]
    fn request_renders_finals_only() {
        let seg = |final_: bool, text: &str| TranscriptSegment {
            side: StreamSide::Inbound,
            seq: 0,
            text: text.into(),
            is_final: final_,
            start_ms: 0,
            end_ms: 1,
            confidence: None,
            latency_ms: 1,
        };
        let req = build_tracker_request(&[seg(true, "done deal"), seg(false, "partial words")]);
        assert!(req.user.contains("done deal"));
        assert!(!req.user.contains("partial words"));
    }
}
