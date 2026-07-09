use thiserror::Error;

/// Unified error type crossing module boundaries. Layer-internal errors are
/// mapped into one of these variants at the boundary so callers (and the IPC
/// layer) never depend on implementation details.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("audio device error: {0}")]
    Audio(String),

    #[error("transcription error: {0}")]
    Asr(String),

    #[error("RAG error: {0}")]
    Rag(String),

    #[error("LLM provider error: {0}")]
    Llm(String),

    #[error("configuration error: {0}")]
    Config(String),
}
