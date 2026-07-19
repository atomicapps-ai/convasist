//! Deepgram cloud streaming transcription (design §4.2, `AsrEngineId::
//! DeepgramCloud` — the cloud opt-in next to local whisper).
//!
//! Why it exists: local whisper latency is bounded by CPU decode speed. The
//! Deepgram live API streams PCM over a WebSocket and returns interim
//! results in ~100–300 ms regardless of the local machine — true
//! conversation speed. Trade-off: audio leaves the machine and it needs an
//! API key + network, so it is strictly opt-in (Settings → engine).
//!
//! Threading: one worker thread per conversation side owns its socket
//! (blocking I/O off the UI/audio paths, §2.4). The socket read timeout is
//! short so a single loop alternates between draining captured frames into
//! the socket and reading result messages — no shared-socket locking.
//! Failure contract: if the connection can't be established the session
//! start falls back to local whisper (handled by the session layer); a
//! mid-session drop attempts one reconnect, then goes quiet rather than
//! killing the session.

use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use convasist_core::asr::TranscriptSegment;
use convasist_core::audio::{AudioFrame, StreamSide, TARGET_SAMPLE_RATE_HZ};
use convasist_core::CoreError;

const KEYRING_SERVICE: &str = "convasist";
const KEYRING_USER: &str = "api-key-deepgram";

/// Live-streaming endpoint tuned for conversation: interim results on,
/// smart formatting, and endpointing at 300 ms of trailing silence.
const DG_URL: &str = "wss://api.deepgram.com/v1/listen?model=nova-2&encoding=linear16&sample_rate=16000&channels=1&interim_results=true&smart_format=true&endpointing=300";

pub fn store_api_key(key: &str) -> Result<(), CoreError> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| CoreError::Asr(e.to_string()))?;
    if key.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CoreError::Asr(e.to_string())),
        }
    } else {
        entry
            .set_password(key)
            .map_err(|e| CoreError::Asr(e.to_string()))
    }
}

pub fn load_api_key() -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.is_empty())
}

/// Deepgram-backed engine with the same surface the session uses on
/// `WhisperEngine`: `set_sink` → `frame_sender` → `finish`.
pub struct DeepgramEngine {
    side: StreamSide,
    api_key: String,
    sink: Option<Box<dyn FnMut(TranscriptSegment) + Send>>,
    tx: Option<Sender<AudioFrame>>,
    worker: Option<JoinHandle<()>>,
}

impl DeepgramEngine {
    pub fn new(side: StreamSide, api_key: String) -> Self {
        Self {
            side,
            api_key,
            sink: None,
            tx: None,
            worker: None,
        }
    }

    pub fn set_sink(&mut self, sink: Box<dyn FnMut(TranscriptSegment) + Send>) {
        self.sink = Some(sink);
    }

    pub fn frame_sender(&mut self) -> Result<Sender<AudioFrame>, CoreError> {
        if let Some(tx) = &self.tx {
            return Ok(tx.clone());
        }
        let sink = self
            .sink
            .take()
            .ok_or_else(|| CoreError::Asr("sink not set before start".into()))?;
        let (tx, rx) = mpsc::channel::<AudioFrame>();
        let side = self.side;
        let key = self.api_key.clone();

        // Connect synchronously so a bad key / no network surfaces to the
        // caller, which then falls back to local whisper.
        let socket = connect(&key)?;

        let worker = std::thread::Builder::new()
            .name(format!("deepgram-{side:?}"))
            .spawn(move || stream_loop(side, key, socket, rx, sink))
            .map_err(|e| CoreError::Asr(format!("spawn deepgram worker: {e}")))?;

        self.tx = Some(tx.clone());
        self.worker = Some(worker);
        Ok(tx)
    }

    pub fn finish(&mut self) -> Result<(), CoreError> {
        self.tx.take(); // disconnects the channel; worker closes the stream
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Ok(())
    }
}

impl Drop for DeepgramEngine {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

type DgSocket = WebSocket<MaybeTlsStream<TcpStream>>;

fn connect(api_key: &str) -> Result<DgSocket, CoreError> {
    let mut request = DG_URL
        .into_client_request()
        .map_err(|e| CoreError::Asr(e.to_string()))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Token {api_key}")
            .parse()
            .map_err(|_| CoreError::Asr("bad api key format".into()))?,
    );
    let (socket, _response) = tungstenite::connect(request)
        .map_err(|e| CoreError::Asr(format!("deepgram connect: {e}")))?;
    // Short read timeout lets one loop both write audio and poll results.
    if let MaybeTlsStream::Rustls(tls) = socket.get_ref() {
        let _ = tls
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(30)));
    }
    Ok(socket)
}

fn pcm16_bytes(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// One loop per side: drain captured frames into the socket, poll for result
/// messages, emit segments. Exits when the frame channel disconnects (session
/// stop) after asking Deepgram to flush the tail.
fn stream_loop(
    side: StreamSide,
    api_key: String,
    mut socket: DgSocket,
    rx: Receiver<AudioFrame>,
    mut sink: Box<dyn FnMut(TranscriptSegment) + Send>,
) {
    let mut seq: u64 = 0;
    let mut reconnected = false;

    'session: loop {
        // 1) Forward all pending audio.
        loop {
            match rx.try_recv() {
                Ok(frame) => {
                    let bytes = pcm16_bytes(&frame.samples);
                    if socket.send(Message::Binary(bytes)).is_err() {
                        // One reconnect attempt per session; then go quiet.
                        if reconnected {
                            break 'session;
                        }
                        reconnected = true;
                        match connect(&api_key) {
                            Ok(s) => socket = s,
                            Err(_) => break 'session,
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Session stop: ask for the final flush, drain briefly.
                    let _ = socket.send(Message::Text(r#"{"type":"CloseStream"}"#.into()));
                    let deadline = std::time::Instant::now() + Duration::from_millis(700);
                    while std::time::Instant::now() < deadline {
                        match socket.read() {
                            Ok(Message::Text(text)) => {
                                emit_results(side, &text, &mut seq, &mut sink)
                            }
                            Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => {
                                break
                            }
                            Ok(_) => {}
                            Err(_) => {}
                        }
                    }
                    break 'session;
                }
            }
        }

        // 2) Poll for results (short timeout keeps audio flowing).
        match socket.read() {
            Ok(Message::Text(text)) => emit_results(side, &text, &mut seq, &mut sink),
            Ok(Message::Ping(payload)) => {
                let _ = socket.send(Message::Pong(payload));
            }
            Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => break 'session,
            Ok(_) => {}
            // Read timeout (WouldBlock) lands here: just loop.
            Err(_) => {}
        }
    }
    let _ = socket.close(None);
}

/// Parse one Deepgram results message and emit it as a transcript segment.
/// Interim results reuse the open `seq` (the UI replaces in place); a final
/// closes it. Kept as a free function for unit testing.
fn emit_results(
    side: StreamSide,
    text: &str,
    seq: &mut u64,
    sink: &mut Box<dyn FnMut(TranscriptSegment) + Send>,
) {
    let Some(segment) = parse_result(side, text, *seq) else {
        return;
    };
    let is_final = segment.is_final;
    if !segment.text.is_empty() {
        sink(segment);
    }
    if is_final {
        *seq += 1;
    }
}

fn parse_result(side: StreamSide, text: &str, seq: u64) -> Option<TranscriptSegment> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if value.get("type").and_then(|t| t.as_str()) != Some("Results") {
        return None;
    }
    let alternative = value
        .get("channel")?
        .get("alternatives")?
        .as_array()?
        .first()?;
    let transcript = alternative.get("transcript")?.as_str()?.trim().to_string();
    let is_final = value
        .get("is_final")
        .and_then(|f| f.as_bool())
        .unwrap_or(false);
    let start_s = value.get("start").and_then(|s| s.as_f64()).unwrap_or(0.0);
    let duration_s = value
        .get("duration")
        .and_then(|d| d.as_f64())
        .unwrap_or(0.0);
    let confidence = alternative
        .get("confidence")
        .and_then(|c| c.as_f64())
        .map(|c| c as f32);

    Some(TranscriptSegment {
        side,
        seq,
        text: transcript,
        is_final,
        start_ms: (start_s * 1000.0) as u64,
        end_ms: ((start_s + duration_s) * 1000.0) as u64,
        confidence,
        // Streaming interims arrive in ~100–300 ms; there is no local decode
        // step to time, so report the transport as instantaneous.
        latency_ms: 0,
    })
}

// Silence the unused-constant lint when tests are compiled without the
// network feature — TARGET_SAMPLE_RATE_HZ documents the PCM contract.
const _: () = assert!(TARGET_SAMPLE_RATE_HZ == 16_000);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_interim_and_final_results() {
        let msg = r#"{
            "type": "Results",
            "start": 1.5, "duration": 0.8, "is_final": false,
            "channel": {"alternatives": [{"transcript": "hello wor", "confidence": 0.82}]}
        }"#;
        let seg = parse_result(StreamSide::Inbound, msg, 3).expect("parse");
        assert_eq!(seg.text, "hello wor");
        assert!(!seg.is_final);
        assert_eq!(seg.seq, 3);
        assert_eq!(seg.start_ms, 1500);
        assert_eq!(seg.end_ms, 2300);
        assert!(seg.confidence.unwrap() > 0.8);

        let final_msg = msg.replace("\"is_final\": false", "\"is_final\": true");
        let seg = parse_result(StreamSide::Inbound, &final_msg, 3).expect("parse");
        assert!(seg.is_final);
    }

    #[test]
    fn ignores_non_result_messages() {
        assert!(parse_result(StreamSide::Inbound, r#"{"type":"Metadata"}"#, 0).is_none());
        assert!(parse_result(StreamSide::Inbound, "not json", 0).is_none());
    }

    #[test]
    fn seq_advances_only_on_finals() {
        use std::sync::{Arc, Mutex};
        let collected: Arc<Mutex<Vec<TranscriptSegment>>> = Arc::default();
        let sink_target = collected.clone();
        let mut seq = 0u64;
        let mut sink: Box<dyn FnMut(TranscriptSegment) + Send> =
            Box::new(move |s| sink_target.lock().unwrap().push(s));
        let interim = r#"{"type":"Results","is_final":false,"start":0,"duration":1,
            "channel":{"alternatives":[{"transcript":"one"}]}}"#;
        let fin = r#"{"type":"Results","is_final":true,"start":0,"duration":1,
            "channel":{"alternatives":[{"transcript":"one two"}]}}"#;
        emit_results(StreamSide::Outbound, interim, &mut seq, &mut sink);
        emit_results(StreamSide::Outbound, interim, &mut seq, &mut sink);
        assert_eq!(seq, 0, "interims keep the seq open");
        emit_results(StreamSide::Outbound, fin, &mut seq, &mut sink);
        assert_eq!(seq, 1, "final closes the seq");
        let collected = collected.lock().unwrap();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0].seq, collected[2].seq);
    }

    #[test]
    fn pcm16_is_little_endian_16bit() {
        let bytes = pcm16_bytes(&[0.0, 1.0, -1.0]);
        assert_eq!(bytes.len(), 6);
        assert_eq!(&bytes[0..2], &[0, 0]);
        assert_eq!(i16::from_le_bytes([bytes[2], bytes[3]]), i16::MAX);
    }
}
