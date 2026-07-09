//! Local embedding model (design §4.4 R2): BGE-small-en-v1.5 via fastembed
//! (ONNX Runtime, CPU). The model (~130 MB) downloads to the app cache on
//! first initialization.
//!
//! Everything here is best-effort by contract: when the embedder is not
//! (yet) available, callers fall back to BM25-only retrieval — hybrid
//! search is an upgrade, never a dependency.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

static EMBEDDER: OnceLock<Option<Mutex<TextEmbedding>>> = OnceLock::new();

/// Initialize (downloading the model if needed) — call from a background
/// thread; the first run can take minutes on a slow connection.
pub fn warm(cache_dir: PathBuf) {
    let _ = EMBEDDER.get_or_init(|| {
        TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15)
                .with_cache_dir(cache_dir)
                .with_show_download_progress(false),
        )
        .ok()
        .map(Mutex::new)
    });
}

/// True once `warm` has completed successfully.
pub fn ready() -> bool {
    matches!(EMBEDDER.get(), Some(Some(_)))
}

/// Embed a batch of passages. `None` when the embedder is unavailable —
/// callers degrade to lexical-only retrieval.
pub fn embed(texts: Vec<String>) -> Option<Vec<Vec<f32>>> {
    let embedder = EMBEDDER.get()?.as_ref()?;
    let guard = embedder.lock().ok()?;
    guard.embed(texts, None).ok()
}

/// Embed a single retrieval query. Only uses an ALREADY-initialized model —
/// never blocks a retrieval path on the first-run download.
pub fn embed_query(query: &str) -> Option<Vec<f32>> {
    if !ready() {
        return None;
    }
    embed(vec![query.to_string()]).and_then(|mut v| v.pop())
}
