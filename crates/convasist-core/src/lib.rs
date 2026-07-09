//! convasist-core — shell-agnostic domain layer.
//!
//! Everything in this crate is pure logic: types, traits, and the IPC
//! contract. It has no Tauri, GUI, or OS-audio dependency, so it builds and
//! tests on any platform. Platform implementations (WASAPI capture, whisper
//! bindings, provider HTTP clients) live behind the traits defined here and
//! are wired up in `src-tauri`.
//!
//! Architecture reference: docs/phase-1-design-and-spec.md §3 (module
//! boundaries) and §2.4 (threading & data-flow contract).

pub mod asr;
pub mod audio;
pub mod bm25;
pub mod chunk;
pub mod config;
pub mod dsp;
pub mod error;
pub mod ipc;
pub mod llm;
pub mod prompt;
pub mod radar;
pub mod rag;
pub mod tracker;
pub mod vad;

pub use error::CoreError;
