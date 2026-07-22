//! Commitment & entity tracker worker (design §6.3).
//!
//! One thread per live session. It buffers finalized segments and runs a
//! fast-slot LLM extraction pass when enough new speech accumulates (≥5
//! finals, or ≥2 finals and ≥45 s since the last pass). Results merge into
//! a session-scoped deduped state, re-emitted as a full TRACKER event.
//!
//! Everything is best-effort: an extraction failure skips silently — the
//! tracker is an enhancement, never a blocker.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use convasist_core::asr::TranscriptSegment;
use convasist_core::ipc::{events, TrackerEvent};
use convasist_core::llm::ModelSelection;
use convasist_core::tracker::{
    build_tracker_request, parse_tracker_reply, TrackedCommitment, TrackedEntity,
};

const POLL: Duration = Duration::from_secs(5);
const MIN_BATCH: usize = 5;
const IDLE_BATCH: usize = 2;
const IDLE_AFTER: Duration = Duration::from_secs(45);

/// Spawn the worker; returns the sender for finalized segments. Dropping
/// every sender (session stop) triggers one last pass and shuts it down.
pub fn spawn_tracker(
    app: AppHandle,
    selection: ModelSelection,
    api_key: String,
) -> Sender<TranscriptSegment> {
    let (tx, rx) = std::sync::mpsc::channel::<TranscriptSegment>();
    let _ = std::thread::Builder::new()
        .name("tracker".into())
        .spawn(move || worker(app, selection, api_key, rx));
    tx
}

struct TrackerState {
    entities: Vec<TrackedEntity>,
    commitments: Vec<TrackedCommitment>,
    seen: HashSet<String>,
}

impl TrackerState {
    fn new() -> Self {
        Self {
            entities: Vec::new(),
            commitments: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn merge(&mut self, extraction: convasist_core::tracker::TrackerExtraction) -> bool {
        let mut changed = false;
        for entity in extraction.entities {
            let key = format!("e:{}", entity.label.trim().to_lowercase());
            if entity.label.trim().is_empty() || !self.seen.insert(key) {
                continue;
            }
            self.entities.push(entity);
            changed = true;
        }
        for commitment in extraction.commitments {
            let key = format!(
                "c:{}:{}",
                commitment.who.trim().to_lowercase(),
                commitment.what.trim().to_lowercase()
            );
            if commitment.what.trim().is_empty() || !self.seen.insert(key) {
                continue;
            }
            self.commitments.push(commitment);
            changed = true;
        }
        changed
    }
}

fn worker(
    app: AppHandle,
    selection: ModelSelection,
    api_key: String,
    rx: Receiver<TranscriptSegment>,
) {
    let mut buffer: Vec<TranscriptSegment> = Vec::new();
    let mut state = TrackerState::new();
    let mut last_run = Instant::now();

    loop {
        let disconnected = match rx.recv_timeout(POLL) {
            Ok(segment) => {
                if segment.is_final && !segment.text.trim().is_empty() {
                    buffer.push(segment);
                }
                false
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => true,
        };

        let due = buffer.len() >= MIN_BATCH
            || (buffer.len() >= IDLE_BATCH && last_run.elapsed() >= IDLE_AFTER)
            || (disconnected && !buffer.is_empty());

        if due {
            run_extraction(&app, &selection, &api_key, &mut buffer, &mut state);
            last_run = Instant::now();
        }
        if disconnected {
            return;
        }
    }
}

fn run_extraction(
    app: &AppHandle,
    selection: &ModelSelection,
    api_key: &str,
    buffer: &mut Vec<TranscriptSegment>,
    state: &mut TrackerState,
) {
    let request = build_tracker_request(buffer);
    buffer.clear();
    if request.user.trim().is_empty() {
        return;
    }

    let mut reply = String::new();
    let result = crate::llm::stream_completion(
        selection.provider,
        api_key,
        &selection.model,
        &request,
        &mut |token| reply.push_str(token),
    );
    if result.is_err() {
        return; // best-effort: skip this pass
    }
    let Some(extraction) = parse_tracker_reply(&reply) else {
        return;
    };
    if state.merge(extraction) {
        let _ = app.emit(
            events::TRACKER,
            TrackerEvent {
                entities: state.entities.clone(),
                commitments: state.commitments.clone(),
            },
        );
    }
}
