//! Structure-aware document chunking (design §4.4 R1).
//!
//! Targets ~300–500 tokens per chunk (≈1200–2000 chars) with overlap, and
//! carries a heading breadcrumb so retrieval hits can point at "§ Pricing >
//! Premiums" instead of a bare offset.

/// Chunk size targets, in characters (≈4 chars/token).
const TARGET_CHARS: usize = 1_600;
const MAX_CHARS: usize = 2_200;
/// Overlap carried between adjacent chunks of the same section (~15%).
const OVERLAP_CHARS: usize = 240;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Heading breadcrumb ("Pricing > Premiums") or paragraph range.
    pub location: String,
    pub text: String,
}

/// Split markdown-ish or plain text into chunks. Lines starting with `#`
/// update the heading breadcrumb (markdown); documents without headings
/// fall back to paragraph grouping with positional locations.
pub fn chunk_text(text: &str) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    // breadcrumb stack: (level, title)
    let mut crumbs: Vec<(usize, String)> = Vec::new();
    let mut current = String::new();
    let mut paragraph_index = 0usize;
    let mut chunk_start_paragraph = 0usize;

    let breadcrumb = |crumbs: &[(usize, String)], start_p: usize, end_p: usize| -> String {
        if crumbs.is_empty() {
            if start_p == end_p {
                format!("¶{}", start_p + 1)
            } else {
                format!("¶{}–{}", start_p + 1, end_p + 1)
            }
        } else {
            crumbs
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(" > ")
        }
    };

    fn flush(chunks: &mut Vec<Chunk>, current: &mut String, location: String) {
        let text = current.trim();
        if !text.is_empty() {
            chunks.push(Chunk {
                location,
                text: text.to_string(),
            });
        }
        // Keep a tail as overlap so a fact straddling the boundary appears
        // in both chunks.
        let tail: String = text
            .chars()
            .skip(text.chars().count().saturating_sub(OVERLAP_CHARS))
            .collect();
        current.clear();
        if !tail.is_empty() {
            current.push_str(&tail);
            current.push_str("\n\n");
        }
    }

    for paragraph in text.split("\n\n") {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Heading? Close the running chunk (no overlap across sections).
        if let Some(heading) = parse_heading(trimmed) {
            let text_now = current.trim().to_string();
            if !text_now.is_empty() {
                chunks.push(Chunk {
                    location: breadcrumb(&crumbs, chunk_start_paragraph, paragraph_index),
                    text: text_now,
                });
            }
            current.clear();
            let (level, title) = heading;
            crumbs.retain(|(l, _)| *l < level);
            crumbs.push((level, title));
            chunk_start_paragraph = paragraph_index;
            continue;
        }

        if current.trim().is_empty() {
            chunk_start_paragraph = paragraph_index;
        }
        current.push_str(trimmed);
        current.push_str("\n\n");
        paragraph_index += 1;

        // Pathological single paragraph far beyond max: hard-split first so
        // no emitted chunk ever exceeds MAX_CHARS.
        while current.len() > MAX_CHARS {
            let split_at = current
                .char_indices()
                .nth(TARGET_CHARS)
                .map(|(i, _)| i)
                .unwrap_or(current.len());
            if split_at == 0 || split_at >= current.len() {
                break;
            }
            let rest = current.split_off(split_at);
            chunks.push(Chunk {
                location: breadcrumb(&crumbs, chunk_start_paragraph, paragraph_index - 1),
                text: current.trim().to_string(),
            });
            current = rest;
        }

        if current.len() >= TARGET_CHARS {
            let location = breadcrumb(&crumbs, chunk_start_paragraph, paragraph_index - 1);
            flush(&mut chunks, &mut current, location);
            chunk_start_paragraph = paragraph_index;
        }
    }

    let text_now = current.trim().to_string();
    if !text_now.is_empty() {
        chunks.push(Chunk {
            location: breadcrumb(&crumbs, chunk_start_paragraph, paragraph_index.max(1) - 1),
            text: text_now,
        });
    }
    chunks
}

/// `## Title` → (2, "Title"). Only markdown ATX headings count.
fn parse_heading(paragraph: &str) -> Option<(usize, String)> {
    // Headings are single-line by definition.
    let line = paragraph.lines().next()?;
    if paragraph.lines().count() > 1 {
        return None;
    }
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let title = line[hashes..].trim();
    if title.is_empty() {
        return None;
    }
    Some((hashes, title.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_no_chunks() {
        assert!(chunk_text("").is_empty());
        assert!(chunk_text("\n\n  \n\n").is_empty());
    }

    #[test]
    fn headings_become_breadcrumbs() {
        let doc =
            "# Pricing\n\n## Premiums\n\nThe premium is $120.\n\n## Discounts\n\nBundles save 10%.";
        let chunks = chunk_text(doc);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].location, "Pricing > Premiums");
        assert!(chunks[0].text.contains("$120"));
        assert_eq!(chunks[1].location, "Pricing > Discounts");
    }

    #[test]
    fn heading_levels_pop_correctly() {
        let doc = "# A\n\n## B\n\ncontent b\n\n# C\n\ncontent c";
        let chunks = chunk_text(doc);
        assert_eq!(chunks[0].location, "A > B");
        assert_eq!(chunks[1].location, "C");
    }

    #[test]
    fn long_sections_split_with_overlap() {
        let paragraph = "word ".repeat(120); // ~600 chars
        let doc = format!("# Long\n\n{}", vec![paragraph; 8].join("\n\n"));
        let chunks = chunk_text(&doc);
        assert!(chunks.len() >= 2, "got {} chunks", chunks.len());
        for c in &chunks {
            assert!(c.text.len() <= MAX_CHARS + OVERLAP_CHARS);
            assert_eq!(c.location, "Long");
        }
    }

    #[test]
    fn plain_text_gets_paragraph_locations() {
        let chunks = chunk_text("First paragraph.\n\nSecond paragraph.");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].location.starts_with('¶'));
    }

    #[test]
    fn giant_single_paragraph_hard_splits() {
        let doc = "x".repeat(10_000);
        let chunks = chunk_text(&doc);
        assert!(chunks.len() >= 4);
        for c in &chunks {
            assert!(c.text.len() <= MAX_CHARS);
        }
    }
}
