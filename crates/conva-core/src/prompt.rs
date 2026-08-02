//! Ally context builder (design §4.5 O1).
//!
//! Assembles the LLM request from the transcript window (both sides,
//! chronological) plus optional RAG chunks, under a hard character budget
//! (≈4 chars/token). Newest transcript turns win when the budget bites —
//! the freshest context is the most valuable.

use crate::asr::TranscriptSegment;
use crate::audio::StreamSide;
use crate::llm::LlmRequest;
use crate::rag::ScoredChunk;

/// Character budget for the transcript portion of the prompt
/// (≈4k tokens — comfortable for every provider in the registry).
pub const TRANSCRIPT_CHAR_BUDGET: usize = 16_000;
/// Character budget for retrieved reference chunks.
pub const RAG_CHAR_BUDGET: usize = 8_000;

pub const SYSTEM_PROMPT: &str = "You are Ally, conva's strategic intelligence \
ally. You work alongside the user in a live conversation — in real time, on \
their device. THEM lines are what the other party said (system audio), YOU \
lines are what the user said (microphone). Transcripts come from speech \
recognition and may contain small errors — read through them. \
Use the best source for each question. When the user's reference material is \
relevant, lean on it and cite the source you used (e.g. [pricing.pdf]). When \
the material doesn't cover the question, just answer from your own knowledge — \
do NOT refuse or say \"it's not in the documents\" unless the user is \
specifically asking about their own documents. Never fabricate specific facts, \
figures, or citations; if you are genuinely unsure, say so briefly. \
FORMAT for a live call — the user is glancing mid-conversation and must act in \
seconds: lead with the answer as a few short bullet points (fragments, not full \
sentences), and put the single most important fact or instruction in **bold**. \
No preamble, no restating the question, no \"what's happening\" recap. If — and \
only if — deeper background or your read of the situation genuinely adds value, \
put it AFTER a line containing only three dashes (---); everything above the \
dashes must stand alone as the at-a-glance answer, everything below is optional \
context.";

/// What the user wants from Ally (O3 prompt templates, minimal set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllyKind {
    /// Suggest how to respond to the latest exchange.
    SuggestReply,
    /// Summarize the conversation so far.
    Summarize,
    /// Answer a free-form question about the conversation.
    Question,
}

/// Render one transcript line. Finals only — partials are still mutating.
fn render_line(segment: &TranscriptSegment) -> String {
    let speaker = match segment.side {
        StreamSide::Inbound => "THEM",
        StreamSide::Outbound => "YOU",
    };
    format!("{speaker}: {}\n", segment.text.trim())
}

/// Build the request. `segments` must be in chronological order; partials
/// are skipped. `question` is used by `AllyKind::Question`.
pub fn build_ally_request(
    kind: AllyKind,
    segments: &[TranscriptSegment],
    chunks: &[ScoredChunk],
    question: Option<&str>,
    max_tokens: u32,
) -> LlmRequest {
    // Take newest-first until the budget is spent, then restore order.
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0usize;
    for segment in segments.iter().rev() {
        if !segment.is_final || segment.text.trim().is_empty() {
            continue;
        }
        let line = render_line(segment);
        if used + line.len() > TRANSCRIPT_CHAR_BUDGET {
            break;
        }
        used += line.len();
        lines.push(line);
    }
    lines.reverse();
    let transcript: String = lines.concat();

    let mut reference = String::new();
    for chunk in chunks {
        let block = format!(
            "[source: {} — {}]\n{}\n\n",
            chunk.file_name, chunk.location, chunk.text
        );
        if reference.len() + block.len() > RAG_CHAR_BUDGET {
            break;
        }
        reference.push_str(&block);
    }

    let task = match kind {
        AllyKind::SuggestReply => "Suggest how I should respond to the latest exchange. Give 1-3 \
             short talking points I can use right now."
            .to_string(),
        AllyKind::Summarize => "Summarize this conversation so far: key points, any commitments \
             made, and open questions."
            .to_string(),
        AllyKind::Question => question
            .unwrap_or("Help me with this conversation.")
            .to_string(),
    };

    let mut user = String::new();
    if !reference.is_empty() {
        user.push_str("Reference material:\n\n");
        user.push_str(&reference);
    }
    if transcript.is_empty() {
        user.push_str("(No conversation captured yet.)\n\n");
    } else {
        user.push_str("Conversation transcript:\n\n");
        user.push_str(&transcript);
        user.push('\n');
    }
    user.push_str("Task: ");
    user.push_str(&task);

    LlmRequest {
        system: SYSTEM_PROMPT.to_string(),
        user,
        max_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(side: StreamSide, seq: u64, text: &str, is_final: bool) -> TranscriptSegment {
        TranscriptSegment {
            side,
            seq,
            text: text.to_string(),
            is_final,
            start_ms: seq * 1000,
            end_ms: seq * 1000 + 900,
            confidence: None,
            latency_ms: 100,
        }
    }

    #[test]
    fn renders_speakers_in_order_and_skips_partials() {
        let segments = vec![
            segment(StreamSide::Inbound, 0, "How much is the premium?", true),
            segment(StreamSide::Outbound, 0, "Let me check that for you", true),
            segment(StreamSide::Inbound, 1, "still talki", false), // partial
        ];
        let req = build_ally_request(AllyKind::SuggestReply, &segments, &[], None, 512);
        let them = req.user.find("THEM: How much is the premium?").unwrap();
        let you = req.user.find("YOU: Let me check that for you").unwrap();
        assert!(them < you, "chronological order");
        assert!(!req.user.contains("still talki"), "partials excluded");
    }

    #[test]
    fn budget_keeps_newest_turns() {
        let filler = "x".repeat(400);
        let segments: Vec<_> = (0..100)
            .map(|i| segment(StreamSide::Inbound, i, &format!("turn {i} {filler}"), true))
            .collect();
        let req = build_ally_request(AllyKind::Summarize, &segments, &[], None, 512);
        assert!(req.user.len() < TRANSCRIPT_CHAR_BUDGET + 2_000);
        assert!(req.user.contains("turn 99"), "newest turn kept");
        assert!(!req.user.contains("turn 0 "), "oldest turn dropped");
    }

    #[test]
    fn question_kind_uses_the_question() {
        let req = build_ally_request(
            AllyKind::Question,
            &[],
            &[],
            Some("What does HVAC stand for?"),
            512,
        );
        assert!(req.user.contains("What does HVAC stand for?"));
        assert!(req.user.contains("No conversation captured yet"));
    }

    #[test]
    fn rag_chunks_carry_source_attribution() {
        let chunks = vec![crate::rag::ScoredChunk {
            document_id: "d1".into(),
            file_name: "pricing.pdf".into(),
            location: "§2 Premiums".into(),
            text: "The 2026 premium is $120/mo.".into(),
            score: 0.9,
        }];
        let req = build_ally_request(AllyKind::SuggestReply, &[], &chunks, None, 512);
        assert!(req.user.contains("[source: pricing.pdf — §2 Premiums]"));
        assert!(req.user.contains("$120/mo"));
    }
}
