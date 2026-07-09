//! Capture implementation (design §4.1 A1–A4) built on cpal/WASAPI.
//!
//! Threading model per §2.4:
//! - The cpal device callback ONLY copies interleaved samples into a
//!   lock-free SPSC ring buffer (no allocation, no locks, no logging).
//! - A dedicated worker thread owns the cpal stream, drains the ring every
//!   ~50 ms, downmixes + resamples to 16 kHz mono, stamps a monotonic
//!   timestamp, and hands `AudioFrame`s to the sink.
//! - On device error (unplug), the worker drops the stream and retries a
//!   rebuild every second until it succeeds or the session stops (A3
//!   hot-swap recovery).
//!
//! Loopback: on Windows/WASAPI, building an *input* stream on an *output*
//! device captures what that device plays (system audio). Other platforms
//! reject it at runtime — Phase 1 is Windows-only (§9 row 4).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::FromSample;
use rtrb::{Consumer, Producer, RingBuffer};

use convasist_core::audio::{
    AudioDevice, AudioFrame, AudioSource, StreamSide, TARGET_SAMPLE_RATE_HZ,
};
use convasist_core::dsp::{downmix_interleaved_to_mono, LinearResampler};
use convasist_core::CoreError;

const DRAIN_INTERVAL: Duration = Duration::from_millis(50);
const REOPEN_RETRY: Duration = Duration::from_millis(1000);
/// Ring sized for ~2 s of 8-channel 192 kHz audio — far beyond any real
/// device, so the callback never blocks on a slow drain.
const RING_CAPACITY: usize = 1 << 21;

/// Enumerate capture candidates for the device picker (A3).
/// Microphones capture the outbound side; output devices are loopback
/// (inbound) candidates.
pub fn list_devices() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    let default_in = host.default_input_device().and_then(|d| d.name().ok());
    if let Ok(inputs) = host.input_devices() {
        for d in inputs {
            if let Ok(name) = d.name() {
                devices.push(AudioDevice {
                    id: name.clone(),
                    is_default: Some(&name) == default_in.as_ref(),
                    name,
                    side: StreamSide::Outbound,
                });
            }
        }
    }

    let default_out = host.default_output_device().and_then(|d| d.name().ok());
    if let Ok(outputs) = host.output_devices() {
        for d in outputs {
            if let Ok(name) = d.name() {
                devices.push(AudioDevice {
                    id: name.clone(),
                    is_default: Some(&name) == default_out.as_ref(),
                    name,
                    side: StreamSide::Inbound,
                });
            }
        }
    }

    devices
}

enum Ctrl {
    Stop,
    Reopen,
}

/// One capture source (mic or loopback) implementing the core contract.
pub struct CpalSource {
    side: StreamSide,
    /// Device name to open; `None` = platform default for the side.
    device_name: Option<String>,
    ctrl: Option<Sender<Ctrl>>,
    worker: Option<JoinHandle<()>>,
}

impl CpalSource {
    pub fn new(side: StreamSide, device_name: Option<String>) -> Self {
        Self {
            side,
            device_name,
            ctrl: None,
            worker: None,
        }
    }
}

impl AudioSource for CpalSource {
    fn side(&self) -> StreamSide {
        self.side
    }

    fn start(&mut self, sink: Box<dyn FnMut(AudioFrame) + Send>) -> Result<(), CoreError> {
        let (ctrl_tx, ctrl_rx) = mpsc::channel::<Ctrl>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), CoreError>>();

        let side = self.side;
        let device_name = self.device_name.clone();
        let err_ctrl = ctrl_tx.clone();

        let worker = std::thread::Builder::new()
            .name(format!("audio-{side:?}"))
            .spawn(move || worker_main(side, device_name, ctrl_rx, err_ctrl, ready_tx, sink))
            .map_err(|e| CoreError::Audio(format!("spawn worker: {e}")))?;

        // Surface stream-open failures synchronously to the caller.
        let startup = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| CoreError::Audio("capture worker did not start".into()))?;
        if startup.is_err() {
            let _ = worker.join();
            return startup;
        }

        self.ctrl = Some(ctrl_tx);
        self.worker = Some(worker);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CoreError> {
        if let Some(ctrl) = self.ctrl.take() {
            let _ = ctrl.send(Ctrl::Stop);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Ok(())
    }
}

impl Drop for CpalSource {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct OpenStream {
    // Held for its Drop (closes the device); never accessed after build.
    _stream: cpal::Stream,
    consumer: Consumer<f32>,
    channels: usize,
    resampler: LinearResampler,
}

fn resolve_device(
    side: StreamSide,
    device_name: &Option<String>,
) -> Result<cpal::Device, CoreError> {
    let host = cpal::default_host();
    let default = match side {
        StreamSide::Outbound => host.default_input_device(),
        StreamSide::Inbound => host.default_output_device(),
    };
    let device = match device_name {
        None => default,
        Some(wanted) => {
            let iter = match side {
                StreamSide::Outbound => host.input_devices(),
                StreamSide::Inbound => host.output_devices(),
            }
            .map_err(|e| CoreError::Audio(format!("enumerate devices: {e}")))?;
            iter.into_iter()
                .find(|d| d.name().map(|n| &n == wanted).unwrap_or(false))
                // Configured device unplugged → fall back to default (A3).
                .or(default)
        }
    };
    device.ok_or_else(|| CoreError::Audio(format!("no {side:?} device available")))
}

fn open_stream(
    side: StreamSide,
    device_name: &Option<String>,
    err_ctrl: Sender<Ctrl>,
) -> Result<OpenStream, CoreError> {
    let device = resolve_device(side, device_name)?;

    // For loopback (inbound), WASAPI exposes the render device's mix format
    // through the *input* config path; fall back to the output config.
    let config = device
        .default_input_config()
        .or_else(|_| device.default_output_config())
        .map_err(|e| CoreError::Audio(format!("query device config: {e}")))?;

    let channels = config.channels() as usize;
    let sample_rate = config.sample_rate().0;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    let (producer, consumer) = RingBuffer::<f32>::new(RING_CAPACITY);

    let err_cb = move |_e: cpal::StreamError| {
        // Device vanished / invalidated: ask the worker to rebuild (A3).
        let _ = err_ctrl.send(Ctrl::Reopen);
    };

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, producer, err_cb),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, producer, err_cb),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, producer, err_cb),
        other => Err(CoreError::Audio(format!(
            "unsupported sample format {other:?}"
        ))),
    }?;

    stream
        .play()
        .map_err(|e| CoreError::Audio(format!("start stream: {e}")))?;

    Ok(OpenStream {
        _stream: stream,
        consumer,
        channels,
        resampler: LinearResampler::new(sample_rate, TARGET_SAMPLE_RATE_HZ),
    })
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut producer: Producer<f32>,
    err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, CoreError>
where
    T: cpal::SizedSample + cpal::Sample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                // §2.4 rule 1: copy into the ring and nothing else. If the
                // ring is full (drain stalled), samples drop here — partials
                // may glitch but the callback never blocks.
                if let Ok(chunk) = producer.write_chunk_uninit(data.len().min(producer.slots())) {
                    chunk.fill_from_iter(data.iter().map(|s| f32::from_sample_(*s)));
                }
            },
            err_cb,
            None,
        )
        .map_err(|e| CoreError::Audio(format!("build stream: {e}")))
}

fn worker_main(
    side: StreamSide,
    device_name: Option<String>,
    ctrl_rx: Receiver<Ctrl>,
    err_ctrl: Sender<Ctrl>,
    ready_tx: Sender<Result<(), CoreError>>,
    mut sink: Box<dyn FnMut(AudioFrame) + Send>,
) {
    let epoch = Instant::now();
    let mut open = match open_stream(side, &device_name, err_ctrl.clone()) {
        Ok(open) => {
            let _ = ready_tx.send(Ok(()));
            Some(open)
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };

    let mut interleaved = Vec::new();
    let mut mono = Vec::new();

    loop {
        match ctrl_rx.recv_timeout(DRAIN_INTERVAL) {
            Ok(Ctrl::Stop) => return,
            Ok(Ctrl::Reopen) => {
                // Release the broken device before rebuilding.
                drop(open.take());
                // Drain any queued duplicate Reopen requests.
                while let Ok(msg) = ctrl_rx.try_recv() {
                    if matches!(msg, Ctrl::Stop) {
                        return;
                    }
                }
                loop {
                    match open_stream(side, &device_name, err_ctrl.clone()) {
                        Ok(o) => {
                            open = Some(o);
                            break;
                        }
                        Err(_) => match ctrl_rx.recv_timeout(REOPEN_RETRY) {
                            Ok(Ctrl::Stop) => return,
                            Ok(Ctrl::Reopen) | Err(RecvTimeoutError::Timeout) => continue,
                            Err(RecvTimeoutError::Disconnected) => return,
                        },
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let Some(stream) = open.as_mut() else {
                    continue;
                };

                let available = stream.consumer.slots();
                if available == 0 {
                    continue;
                }
                interleaved.clear();
                if let Ok(chunk) = stream.consumer.read_chunk(available) {
                    let (a, b) = chunk.as_slices();
                    interleaved.extend_from_slice(a);
                    interleaved.extend_from_slice(b);
                    chunk.commit_all();
                }

                downmix_interleaved_to_mono(&interleaved, stream.channels, &mut mono);
                let mut samples = Vec::new();
                stream.resampler.process(&mono, &mut samples);
                if samples.is_empty() {
                    continue;
                }

                sink(AudioFrame {
                    side,
                    captured_at_ns: epoch.elapsed().as_nanos() as u64,
                    samples,
                });
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}
