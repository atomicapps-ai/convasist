//! Hybrid-retrieval math (design §4.4 R3): cosine similarity for the
//! vector half and reciprocal-rank fusion to combine rankings.
//!
//! Brute-force cosine over a reference library (thousands of 384-dim
//! chunks) is a few million flops — microseconds, far inside the §2.5
//! budget. A dedicated ANN store earns its place only when corpora grow
//! orders of magnitude beyond that.

/// Cosine similarity in [-1, 1]; 0.0 for empty or zero-norm vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

/// Top-k indices by cosine similarity to `query`, best first. Entries with
/// empty vectors (not yet embedded) are skipped.
pub fn top_k_cosine<'a>(
    query: &[f32],
    vectors: impl Iterator<Item = (usize, &'a [f32])>,
    k: usize,
) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = vectors
        .filter(|(_, v)| !v.is_empty())
        .map(|(i, v)| (i, cosine_similarity(query, v)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

/// Standard RRF constant.
pub const RRF_K: f32 = 60.0;

/// Reciprocal-rank fusion: each ranking contributes 1/(RRF_K + rank) per
/// item; missing items simply contribute nothing. Returns top-k fused
/// `(item, score)`, best first.
pub fn rrf_fuse(rankings: &[Vec<usize>], k: usize) -> Vec<(usize, f32)> {
    use std::collections::HashMap;
    let mut scores: HashMap<usize, f32> = HashMap::new();
    for ranking in rankings {
        for (rank, item) in ranking.iter().enumerate() {
            *scores.entry(*item).or_insert(0.0) += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
    }
    let mut fused: Vec<(usize, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused.truncate(k);
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_basics() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert_eq!(
            cosine_similarity(&[1.0], &[1.0, 2.0]),
            0.0,
            "length mismatch"
        );
    }

    #[test]
    fn top_k_skips_unembedded_and_ranks() {
        let vectors: Vec<(usize, Vec<f32>)> = vec![
            (0, vec![1.0, 0.0]),
            (1, vec![]), // not embedded
            (2, vec![0.9, 0.1]),
            (3, vec![0.0, 1.0]),
        ];
        let hits = top_k_cosine(
            &[1.0, 0.0],
            vectors.iter().map(|(i, v)| (*i, v.as_slice())),
            2,
        );
        assert_eq!(hits[0].0, 0);
        assert_eq!(hits[1].0, 2);
    }

    #[test]
    fn rrf_prefers_items_ranked_in_both_lists() {
        // Item 5 is mid-ranked in both lists; items 1 and 9 top one list each.
        let lexical = vec![1, 5, 2];
        let semantic = vec![9, 5, 3];
        let fused = rrf_fuse(&[lexical, semantic], 5);
        assert_eq!(fused[0].0, 5, "consensus item wins: {fused:?}");
    }

    #[test]
    fn rrf_single_ranking_preserves_order() {
        let fused = rrf_fuse(&[vec![7, 3, 1]], 3);
        let order: Vec<usize> = fused.iter().map(|(i, _)| *i).collect();
        assert_eq!(order, vec![7, 3, 1]);
    }

    #[test]
    fn rrf_empty_is_empty() {
        assert!(rrf_fuse(&[], 5).is_empty());
        assert!(rrf_fuse(&[vec![]], 5).is_empty());
    }
}
