//! Question Radar (design §6.2): detect questions/asks in the inbound
//! stream so the shell can surface matching reference chunks instantly —
//! no LLM call, no cost, <15 ms.
//!
//! This is a heuristic, deliberately: it gates a zero-cost UI hint, so a
//! false positive shows a harmless card and a false negative loses nothing.

const INTERROGATIVES: &[&str] = &[
    "what", "how", "why", "when", "where", "who", "which", "whose", "can", "could", "would",
    "will", "do", "does", "did", "is", "are", "was", "were", "should", "shall", "have", "has",
    "may", "might",
];

const ASK_PHRASES: &[&str] = &[
    "tell me",
    "walk me through",
    "explain",
    "i'm wondering",
    "i am wondering",
    "i'd like to know",
    "i want to know",
    "curious about",
    "what about",
    "how about",
];

/// Does this (inbound, finalized) utterance look like a question or an ask
/// directed at the user?
pub fn looks_like_question(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 8 {
        return false;
    }
    let lower = trimmed.to_lowercase();

    if trimmed.ends_with('?') {
        return true;
    }
    if ASK_PHRASES.iter().any(|p| lower.contains(p)) {
        return true;
    }
    // Interrogative-led sentence, even when ASR dropped the question mark
    // (whisper often does on rising intonation).
    let first_word = lower
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .find(|w| !w.is_empty())
        .unwrap_or("");
    INTERROGATIVES.contains(&first_word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_marks_are_questions() {
        assert!(looks_like_question("So what does the premium cover?"));
    }

    #[test]
    fn interrogative_lead_without_mark() {
        // ASR frequently drops the '?' on rising intonation.
        assert!(looks_like_question("how much is the maintenance plan"));
        assert!(looks_like_question("Can you send that over today"));
    }

    #[test]
    fn ask_phrases_count() {
        assert!(looks_like_question(
            "Walk me through the onboarding process"
        ));
        assert!(looks_like_question(
            "I'm wondering about the cancellation policy"
        ));
    }

    #[test]
    fn statements_are_not_questions() {
        assert!(!looks_like_question("We signed the contract yesterday."));
        assert!(!looks_like_question("That sounds good to me, thanks."));
        assert!(!looks_like_question("ok"));
    }
}
