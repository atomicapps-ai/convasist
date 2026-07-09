//! RAG store (design §4.4): ingestion, persistence, and BM25 retrieval.
//!
//! Layout: `<app-data>/rag/<doc_id>.json`, one file per ingested document
//! (metadata + chunks). The BM25 index is rebuilt in memory from enabled
//! documents after every mutation — at reference-library scale that is
//! milliseconds, and it keeps a single source of truth on disk.
//!
//! The vector/embedding half of hybrid retrieval (fastembed + ANN + RRF)
//! joins behind this same interface in a later milestone.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use convasist_core::bm25::Bm25Index;
use convasist_core::chunk::chunk_text;
use convasist_core::rag::{IngestReport, RagDocument, ScoredChunk};
use convasist_core::CoreError;

#[derive(Serialize, Deserialize)]
struct StoredDocument {
    document: RagDocument,
    chunks: Vec<StoredChunk>,
}

#[derive(Serialize, Deserialize)]
struct StoredChunk {
    location: String,
    text: String,
}

/// One searchable chunk reference into the loaded corpus.
struct CorpusEntry {
    document_index: usize,
    chunk_index: usize,
}

pub struct RagStore {
    dir: PathBuf,
    inner: RwLock<Corpus>,
}

#[derive(Default)]
struct Corpus {
    documents: Vec<StoredDocument>,
    entries: Vec<CorpusEntry>,
    index: Option<Bm25Index>,
}

impl RagStore {
    pub fn open(app_data_dir: &Path) -> Result<Self, CoreError> {
        let dir = app_data_dir.join("rag");
        fs::create_dir_all(&dir).map_err(|e| CoreError::Rag(e.to_string()))?;
        let store = Self {
            dir,
            inner: RwLock::new(Corpus::default()),
        };
        store.reload()?;
        Ok(store)
    }

    fn doc_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Load every persisted document and rebuild the BM25 index over the
    /// enabled ones.
    fn reload(&self) -> Result<(), CoreError> {
        let mut documents = Vec::new();
        let entries = fs::read_dir(&self.dir).map_err(|e| CoreError::Rag(e.to_string()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<StoredDocument>(&s).ok())
            {
                Some(doc) => documents.push(doc),
                None => continue, // corrupt file: skip, never brick the library
            }
        }
        documents.sort_by(|a, b| a.document.file_name.cmp(&b.document.file_name));

        let mut corpus_entries = Vec::new();
        let mut texts: Vec<&str> = Vec::new();
        for (document_index, doc) in documents.iter().enumerate() {
            if !doc.document.enabled {
                continue;
            }
            for (chunk_index, chunk) in doc.chunks.iter().enumerate() {
                corpus_entries.push(CorpusEntry {
                    document_index,
                    chunk_index,
                });
                texts.push(&chunk.text);
            }
        }
        let index = Bm25Index::build(texts.into_iter());

        let mut inner = self.inner.write().expect("rag lock");
        *inner = Corpus {
            documents,
            entries: corpus_entries,
            index: Some(index),
        };
        Ok(())
    }

    pub fn ingest(&self, path: &str) -> Result<IngestReport, CoreError> {
        let source = Path::new(path);
        let file_name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();

        let (text, mut warnings) = extract_text(source)?;
        let chunks = chunk_text(&text);
        if chunks.is_empty() {
            return Err(CoreError::Rag(format!(
                "'{file_name}' produced no extractable text"
            )));
        }
        if text.len() < 200 {
            warnings.push("very little text extracted — check the file".into());
        }

        let id = format!("doc-{}", crate::session::now_unix_ms());
        let stored = StoredDocument {
            document: RagDocument {
                id: id.clone(),
                file_name,
                enabled: true,
                chunk_count: chunks.len() as u32,
                ingested_at_unix_ms: crate::session::now_unix_ms(),
            },
            chunks: chunks
                .into_iter()
                .map(|c| StoredChunk {
                    location: c.location,
                    text: c.text,
                })
                .collect(),
        };

        let json = serde_json::to_string(&stored).map_err(|e| CoreError::Rag(e.to_string()))?;
        fs::write(self.doc_path(&id), json).map_err(|e| CoreError::Rag(e.to_string()))?;
        self.reload()?;

        Ok(IngestReport {
            document: stored.document,
            warnings,
        })
    }

    pub fn list(&self) -> Vec<RagDocument> {
        self.inner
            .read()
            .expect("rag lock")
            .documents
            .iter()
            .map(|d| d.document.clone())
            .collect()
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), CoreError> {
        let path = self.doc_path(id);
        let content = fs::read_to_string(&path).map_err(|e| CoreError::Rag(e.to_string()))?;
        let mut stored: StoredDocument =
            serde_json::from_str(&content).map_err(|e| CoreError::Rag(e.to_string()))?;
        stored.document.enabled = enabled;
        let json = serde_json::to_string(&stored).map_err(|e| CoreError::Rag(e.to_string()))?;
        fs::write(&path, json).map_err(|e| CoreError::Rag(e.to_string()))?;
        self.reload()
    }

    pub fn delete(&self, id: &str) -> Result<(), CoreError> {
        match fs::remove_file(self.doc_path(id)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(CoreError::Rag(e.to_string())),
        }
        self.reload()
    }

    /// BM25 top-k over enabled documents' chunks (§2.5: <15 ms budget —
    /// this is in-memory and microseconds at library scale).
    pub fn retrieve(&self, query: &str, k: usize) -> Vec<ScoredChunk> {
        let inner = self.inner.read().expect("rag lock");
        let Some(index) = &inner.index else {
            return Vec::new();
        };
        index
            .search(query, k)
            .into_iter()
            .filter_map(|(entry_index, score)| {
                let entry = inner.entries.get(entry_index)?;
                let doc = inner.documents.get(entry.document_index)?;
                let chunk = doc.chunks.get(entry.chunk_index)?;
                Some(ScoredChunk {
                    document_id: doc.document.id.clone(),
                    file_name: doc.document.file_name.clone(),
                    location: chunk.location.clone(),
                    text: chunk.text.clone(),
                    score,
                })
            })
            .collect()
    }
}

/// Extract plain text from a supported document. Returns (text, warnings).
fn extract_text(path: &Path) -> Result<(String, Vec<String>), CoreError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "txt" | "md" | "markdown" => {
            let text = fs::read_to_string(path).map_err(|e| CoreError::Rag(e.to_string()))?;
            Ok((text, Vec::new()))
        }
        "html" | "htm" => {
            let html = fs::read_to_string(path).map_err(|e| CoreError::Rag(e.to_string()))?;
            Ok((strip_html(&html), Vec::new()))
        }
        "pdf" => {
            // pdf-extract can panic on malformed files; contain it.
            let owned = path.to_path_buf();
            let result = std::panic::catch_unwind(move || pdf_extract::extract_text(&owned));
            match result {
                Ok(Ok(text)) => Ok((text, Vec::new())),
                Ok(Err(e)) => Err(CoreError::Rag(format!("PDF extraction failed: {e}"))),
                Err(_) => Err(CoreError::Rag("PDF extraction crashed on this file".into())),
            }
        }
        "docx" => extract_docx(path).map(|t| (t, Vec::new())),
        other => Err(CoreError::Rag(format!(
            "unsupported file type '.{other}' (supported: pdf, docx, md, txt, html)"
        ))),
    }
}

/// DOCX = zip containing word/document.xml; paragraphs are <w:p>, text runs
/// are <w:t>. Strip tags, keep paragraph boundaries.
fn extract_docx(path: &Path) -> Result<String, CoreError> {
    let file = fs::File::open(path).map_err(|e| CoreError::Rag(e.to_string()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| CoreError::Rag(e.to_string()))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| CoreError::Rag(format!("not a DOCX: {e}")))?
        .read_to_string(&mut xml)
        .map_err(|e| CoreError::Rag(e.to_string()))?;

    // Paragraph closes become blank lines so chunking sees structure.
    let xml = xml.replace("</w:p>", "</w:p>\n\n");
    Ok(strip_html(&xml))
}

/// Minimal tag stripper for HTML/XML: drops tags, script/style bodies, and
/// decodes the common entities. Not a browser — good enough for reference
/// documents.
fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() / 2);
    let mut chars = input.char_indices().peekable();
    let mut skip_until: Option<&str> = None;

    while let Some((i, c)) = chars.next() {
        if let Some(close) = skip_until {
            if input[i..].starts_with(close) {
                for _ in 0..close.len().saturating_sub(1) {
                    chars.next();
                }
                skip_until = None;
            }
            continue;
        }
        if c == '<' {
            let rest = &input[i..];
            let lower = rest
                .get(..8)
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if lower.starts_with("<script") {
                skip_until = Some("</script>");
            } else if lower.starts_with("<style") {
                skip_until = Some("</style>");
            }
            // Skip to the end of the tag.
            for (_, tc) in chars.by_ref() {
                if tc == '>' {
                    break;
                }
            }
            // Block-ish tags imply a break.
            if lower.starts_with("<p") || lower.starts_with("</p") || lower.starts_with("<br") {
                out.push('\n');
            }
            continue;
        }
        out.push(c);
    }

    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    // Collapse >2 consecutive newlines to exactly one blank line.
    let mut cleaned = String::with_capacity(decoded.len());
    let mut newline_run = 0;
    for c in decoded.chars() {
        if c == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                cleaned.push(c);
            }
        } else {
            newline_run = 0;
            cleaned.push(c);
        }
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_drops_tags_and_scripts() {
        let html = "<html><head><style>p{color:red}</style></head>\
                    <body><p>Hello &amp; welcome</p><script>alert(1)</script>\
                    <p>Second</p></body></html>";
        let text = strip_html(html);
        assert!(text.contains("Hello & welcome"));
        assert!(text.contains("Second"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color:red"));
    }

    #[test]
    fn store_roundtrip_ingest_list_retrieve_delete() {
        let dir = std::env::temp_dir().join(format!("convasist-rag-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Write a small markdown file to ingest.
        let doc_path = dir.join("pricing.md");
        fs::write(
            &doc_path,
            "# Pricing\n\nThe maintenance plan costs $90 per year.\n\n# Hours\n\nOpen 8-5 weekdays.",
        )
        .unwrap();

        let store = RagStore::open(&dir).unwrap();
        let report = store.ingest(doc_path.to_str().unwrap()).unwrap();
        assert!(report.document.chunk_count >= 2);

        let docs = store.list();
        assert_eq!(docs.len(), 1);

        let hits = store.retrieve("how much does the maintenance plan cost", 3);
        assert!(!hits.is_empty());
        assert!(hits[0].text.contains("$90"));
        assert_eq!(hits[0].location, "Pricing");

        // Disabled documents drop out of retrieval.
        store.set_enabled(&docs[0].id, false).unwrap();
        assert!(store.retrieve("maintenance plan", 3).is_empty());

        store.delete(&docs[0].id).unwrap();
        assert!(store.list().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
