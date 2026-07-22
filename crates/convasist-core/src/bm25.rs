//! In-memory BM25 retrieval (design §4.4 R3 — the lexical half of hybrid
//! search; the vector half joins it via reciprocal-rank fusion in a later
//! milestone behind the same `RagStore` seam).
//!
//! Scale check: a reference library is dozens of documents → thousands of
//! chunks. BM25 over that is microseconds — far inside the <15 ms retrieval
//! budget (§2.5).

use std::collections::HashMap;

const K1: f32 = 1.2;
const B: f32 = 0.75;

/// Lowercased alphanumeric tokens; everything else is a separator.
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(|t| t.to_string())
        .collect()
}

pub struct Bm25Index {
    /// term → [(doc_index, term_frequency)]
    postings: HashMap<String, Vec<(usize, u32)>>,
    doc_lengths: Vec<u32>,
    average_length: f32,
}

impl Bm25Index {
    pub fn build<'a>(documents: impl Iterator<Item = &'a str>) -> Self {
        let mut postings: HashMap<String, Vec<(usize, u32)>> = HashMap::new();
        let mut doc_lengths = Vec::new();

        for (index, text) in documents.enumerate() {
            let tokens = tokenize(text);
            doc_lengths.push(tokens.len() as u32);
            let mut frequencies: HashMap<String, u32> = HashMap::new();
            for token in tokens {
                *frequencies.entry(token).or_insert(0) += 1;
            }
            for (term, tf) in frequencies {
                postings.entry(term).or_default().push((index, tf));
            }
        }

        let average_length = if doc_lengths.is_empty() {
            0.0
        } else {
            doc_lengths.iter().sum::<u32>() as f32 / doc_lengths.len() as f32
        };
        Self {
            postings,
            doc_lengths,
            average_length,
        }
    }

    pub fn len(&self) -> usize {
        self.doc_lengths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.doc_lengths.is_empty()
    }

    /// Top-k `(doc_index, score)` for the query, best first. Documents with
    /// zero overlap are never returned.
    pub fn search(&self, query: &str, k: usize) -> Vec<(usize, f32)> {
        let n = self.doc_lengths.len() as f32;
        if n == 0.0 {
            return Vec::new();
        }
        let mut scores: HashMap<usize, f32> = HashMap::new();

        for term in tokenize(query) {
            let Some(posting) = self.postings.get(&term) else {
                continue;
            };
            let df = posting.len() as f32;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for &(doc, tf) in posting {
                let len_norm =
                    1.0 - B + B * (self.doc_lengths[doc] as f32 / self.average_length.max(1.0));
                let tf = tf as f32;
                let term_score = idf * (tf * (K1 + 1.0)) / (tf + K1 * len_norm);
                *scores.entry(doc).or_insert(0.0) += term_score;
            }
        }

        let mut ranked: Vec<(usize, f32)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(k);
        ranked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(docs: &[&str]) -> Bm25Index {
        Bm25Index::build(docs.iter().copied())
    }

    #[test]
    fn finds_the_relevant_document() {
        let idx = index(&[
            "The furnace maintenance plan costs ninety dollars yearly",
            "Air conditioning repair pricing depends on refrigerant type",
            "Our office is open monday through friday",
        ]);
        let hits = idx.search("how much does the maintenance plan cost", 2);
        assert_eq!(hits[0].0, 0, "furnace plan doc should rank first");
    }

    #[test]
    fn rare_terms_outweigh_common_ones() {
        let idx = index(&[
            "pricing pricing pricing common words",
            "the unique refrigerant certification requirement",
            "pricing and common words again",
        ]);
        let hits = idx.search("refrigerant certification", 3);
        assert_eq!(hits[0].0, 1);
    }

    #[test]
    fn no_overlap_returns_empty() {
        let idx = index(&["alpha beta gamma"]);
        assert!(idx.search("zzz qqq", 5).is_empty());
    }

    #[test]
    fn empty_index_is_safe() {
        let idx = index(&[]);
        assert!(idx.search("anything", 5).is_empty());
        assert!(idx.is_empty());
    }

    #[test]
    fn k_bounds_results() {
        let idx = index(&["cat dog", "cat mouse", "cat bird"]);
        assert_eq!(idx.search("cat", 2).len(), 2);
    }
}
