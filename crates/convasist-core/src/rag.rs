//! RAG / Vector Layer contract (design §4.4).
//!
//! Phase 1 implementation: fastembed (BGE-small) + embedded LanceDB with
//! hybrid vector+BM25 retrieval. This module defines the boundary only.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::CoreError;

/// A document registered in the RAG library (U5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagDocument {
    pub id: String,
    pub file_name: String,
    /// Whether this document participates in retrieval (per-doc toggle, U5).
    pub enabled: bool,
    pub chunk_count: u32,
    pub ingested_at_unix_ms: u64,
}

/// Ingestion outcome reported to the UI (R1/R2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestReport {
    pub document: RagDocument,
    pub warnings: Vec<String>,
}

/// A retrieved chunk with source attribution (R4/R5 — every AI answer shows
/// which chunks grounded it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredChunk {
    pub document_id: String,
    pub file_name: String,
    /// Heading breadcrumb / page reference for click-through (R1 metadata).
    pub location: String,
    pub text: String,
    /// Fused hybrid score (higher is better).
    pub score: f32,
}

/// The retrieval boundary used by the AI orchestrator. Budget: <15 ms for
/// `retrieve` at k=8 on a warm store (§2.5).
#[async_trait]
pub trait RagStore: Send + Sync {
    async fn ingest(&self, path: &str) -> Result<IngestReport, CoreError>;
    async fn list_documents(&self) -> Result<Vec<RagDocument>, CoreError>;
    async fn set_enabled(&self, document_id: &str, enabled: bool) -> Result<(), CoreError>;
    async fn delete(&self, document_id: &str) -> Result<(), CoreError>;
    async fn retrieve(&self, query: &str, k: usize) -> Result<Vec<ScoredChunk>, CoreError>;
}
