//! Live Sim Con rehearsal (Phase E) — the turn-taking engine.
//!
//! The session layer runs mic-only capture + STT and feeds each finalized user
//! utterance here. This worker detects end-of-turn (a pause after the user
//! stops, or a manual "your turn"), asks the LLM to reply **in character** as
//! the chosen persona (grounded in the Sim Con's knowledge base), streams that
//! reply to the UI as inbound ("THEM") transcript segments, and speaks it with
//! Deepgram Aura. Then it listens again.
//!
//! Runs on its own thread; all blocking I/O (LLM stream, TTS) lives here, never
//! on the UI or audio path. Feedback guard: while the AI is speaking (and the
//! worker is busy), any mic segments that arrive are drained and ignored, so an
//! open-speaker setup doesn't let the AI transcribe its own voice. A headset is
//! still recommended.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use conva_core::asr::TranscriptSegment;
use conva_core::audio::StreamSide;
use conva_core::ipc::{events, RehearsalStateEvent};
use conva_core::llm::{LlmRequest, ModelSelection};
use conva_core::simcon::{persona_live_prompt, KnowledgeProfile, SimConPersona, SimConSession};

use crate::rag::RagStore;
use crate::session::now_unix_ms;

/// Poll cadence while gathering a user turn.
const POLL: Duration = Duration::from_millis(150);
/// Silence after the user's last utterance that ends their turn.
const TURN_SILENCE: Duration = Duration::from_millis(1_300);
/// Cap on a single spoken persona turn (tokens ≈ short speech).
const REPLY_MAX_TOKENS: u32 = 320;

/// Everything the worker needs that the session layer doesn't own.
pub struct RehearsalContext {
    pub selection: ModelSelection,
    pub llm_key: String,
    /// Deepgram key for Aura TTS; `None` → text-only rehearsal (no voice).
    pub tts_key: Option<String>,
    pub session: SimConSession,
    pub profile: KnowledgeProfile,
    pub persona: SimConPersona,
    /// Epoch-ms the session started — base for the transcript timeline so
    /// persona turns interleave correctly with the user's spoken turns.
    pub session_start_ms: u64,
}

fn emit_phase(app: &AppHandle, phase: RehearsalStateEvent) {
    let _ = app.emit(events::REHEARSAL_STATE, phase);
}

/// Spawn the rehearsal worker. `rx` carries finalized user (outbound) segments;
/// `stop` is the session's stop flag; `force_end` is set by the "your turn"
/// command to end the current turn immediately.
pub fn spawn(
    app: AppHandle,
    rag: Arc<RagStore>,
    rx: Receiver<TranscriptSegment>,
    stop: Arc<AtomicBool>,
    force_end: Arc<AtomicBool>,
    ctx: RehearsalContext,
) {
    let _ = std::thread::Builder::new()
        .name("rehearsal".into())
        .spawn(move || run(app, rag, rx, stop, force_end, ctx));
}

fn run(
    app: AppHandle,
    rag: Arc<RagStore>,
    rx: Receiver<TranscriptSegment>,
    stop: Arc<AtomicBool>,
    force_end: Arc<AtomicBool>,
    ctx: RehearsalContext,
) {
    // Full running transcript (both sides) for LLM context. The user side is
    // also emitted to the UI by the capture sink; the persona side is emitted
    // here.
    let mut transcript: Vec<TranscriptSegment> = Vec::new();
    let mut inbound_seq: u64 = 0;

    // The counterparty opens the conversation so the user isn't met with
    // silence (an interviewer greets first, etc.).
    respond(&app, &rag, &ctx, &mut transcript, &mut inbound_seq);

    while !stop.load(Ordering::Relaxed) {
        emit_phase(&app, RehearsalStateEvent::Listening);
        let Some(turn) = gather_turn(&rx, &stop, &force_end) else {
            break; // channel closed or session stopping
        };
        if turn.is_empty() {
            continue;
        }
        transcript.extend(turn);
        respond(&app, &rag, &ctx, &mut transcript, &mut inbound_seq);
        // Drop anything captured while the AI was speaking (echo guard).
        while rx.try_recv().is_ok() {}
    }
    emit_phase(&app, RehearsalStateEvent::Ended);
}

/// Collect one user turn: accumulate finalized utterances until the user has
/// been silent for [`TURN_SILENCE`] or pressed "your turn". Returns `None` when
/// the session is stopping or the channel closed.
fn gather_turn(
    rx: &Receiver<TranscriptSegment>,
    stop: &Arc<AtomicBool>,
    force_end: &Arc<AtomicBool>,
) -> Option<Vec<TranscriptSegment>> {
    let mut turn: Vec<TranscriptSegment> = Vec::new();
    let mut silence = Duration::ZERO;
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match rx.recv_timeout(POLL) {
            Ok(seg) => {
                if seg.text.trim().is_empty() {
                    continue;
                }
                turn.push(seg);
                silence = Duration::ZERO;
                // Fresh speech cancels a stale manual-advance request.
                force_end.store(false, Ordering::Relaxed);
            }
            Err(RecvTimeoutError::Timeout) => {
                if turn.is_empty() {
                    // Nothing said yet: clear a spurious manual advance.
                    force_end.store(false, Ordering::Relaxed);
                    continue;
                }
                silence += POLL;
                if silence >= TURN_SILENCE || force_end.load(Ordering::Relaxed) {
                    force_end.store(false, Ordering::Relaxed);
                    return Some(turn);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Generate + stream + speak one persona turn, appending it to `transcript`.
fn respond(
    app: &AppHandle,
    rag: &RagStore,
    ctx: &RehearsalContext,
    transcript: &mut Vec<TranscriptSegment>,
    inbound_seq: &mut u64,
) {
    // Ground on the user's latest turn (fall back to the Sim Con's purpose so
    // the opening line still has context).
    let query = transcript
        .iter()
        .rev()
        .find(|s| s.side == StreamSide::Outbound)
        .map(|s| s.text.clone())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| ctx.session.purpose.clone());
    let chunks = if query.trim().is_empty() {
        Vec::new()
    } else {
        // Ground the persona on this Sim Con's own knowledge base.
        rag.retrieve_scoped(&query, 6, &ctx.profile.doc_ids)
    };

    let request: LlmRequest = persona_live_prompt(
        &ctx.session,
        &ctx.persona,
        &ctx.profile.research,
        transcript.as_slice(),
        &chunks,
        REPLY_MAX_TOKENS,
    );

    emit_phase(app, RehearsalStateEvent::Thinking);

    *inbound_seq += 1;
    let seq = *inbound_seq;
    // Timestamp on the shared session clock so the persona's turn sorts after
    // the user's latest turn in the cockpit (which orders by start_ms).
    let ts = now_unix_ms().saturating_sub(ctx.session_start_ms);
    let emit = |text: &str, is_final: bool| {
        let _ = app.emit(
            events::TRANSCRIPT_SEGMENT,
            TranscriptSegment {
                side: StreamSide::Inbound,
                seq,
                text: text.to_string(),
                is_final,
                start_ms: ts,
                end_ms: ts,
                confidence: None,
                latency_ms: 0,
            },
        );
    };

    let mut reply = String::new();
    let result = crate::llm::stream_completion(
        ctx.selection.provider,
        &ctx.llm_key,
        &ctx.selection.model,
        &request,
        &mut |tok| {
            reply.push_str(tok);
            emit(&reply, false);
        },
    );
    match result {
        Ok(usage) => crate::metering::record_llm(app, ctx.selection.provider, usage),
        Err(e) => {
            emit(&format!("(couldn't respond: {e})"), true);
            return;
        }
    }
    emit(&reply, true);

    let final_seg = TranscriptSegment {
        side: StreamSide::Inbound,
        seq,
        text: reply.clone(),
        is_final: true,
        start_ms: ts,
        end_ms: ts,
        confidence: None,
        latency_ms: 0,
    };
    // Write the persona turn to the session log (it bypasses the capture sink)
    // so the per-session transcript is complete.
    app.state::<crate::AppState>()
        .session
        .log_segment(&final_seg);
    transcript.push(final_seg);

    // Speak it (best-effort; text still shows if TTS is unavailable).
    if let Some(tts_key) = &ctx.tts_key {
        if !reply.trim().is_empty() {
            emit_phase(app, RehearsalStateEvent::Speaking);
            match crate::tts::speak(tts_key, &reply) {
                Ok(()) => crate::metering::record_tts_characters(app, reply.chars().count() as u64),
                Err(e) => eprintln!("[rehearsal] tts failed: {e}"),
            }
        }
    }
}
