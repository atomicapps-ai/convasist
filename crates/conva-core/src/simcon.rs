//! SimCon — Simulated Conversation: the data model.
//!
//! A **SimCon** is a rehearsal of a high-stakes call (interview, financial
//! review, pitch). The user sets a name + purpose + category and attaches
//! library documents (or asks Ally to generate context); an async pipeline
//! builds a reusable [`KnowledgeProfile`] (library docs + bounded web research,
//! embedded into the RAG store); the AI generates [`SimConPersona`] options and
//! the user picks one; then a real-time session runs the chosen counterparty.
//!
//! Distinct from `conversations` (saved real transcripts) and `session`
//! (per-listen JSONL) — a finished SimCon **saves as** a `Conversation`, and a
//! [`KnowledgeProfile`] can be reattached to a future SimCon *or* a live call.
//!
//! These are pure data types (no OS/storage deps), mirrored to TypeScript in
//! `src/lib/ipc.ts`. Persistence + the async pipeline live in the shell
//! (`src-tauri/src/simcon.rs`, Phase A.2).

use serde::{Deserialize, Serialize};

use crate::asr::TranscriptSegment;
use crate::audio::StreamSide;
use crate::llm::LlmRequest;
use crate::rag::ScoredChunk;

/// The kind of call being rehearsed — drives persona generation + the web
/// research prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimConCategory {
    Interview,
    FinancialReview,
    PerformanceReview,
    SalesPitch,
    Other,
}

/// Lifecycle of a SimCon, start to finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimConStatus {
    /// Being set up (Step 1).
    Draft,
    /// Building the knowledge profile — ingest + web research (Step 2).
    Ingesting,
    /// Personas generated, awaiting the user's Start (Step 3).
    Ready,
    /// A live simulated session is in progress (Step 4).
    Running,
    /// The session finished; its transcript is saved as a `Conversation`.
    Ended,
}

/// One generated counterparty persona/strategy option (three per session,
/// Step 3), one of which the AI flags [`recommended`](Self::recommended).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimConPersona {
    pub id: String,
    /// e.g. "Highly technical & skeptical CFO".
    pub title: String,
    /// One-paragraph summary of how this counterparty behaves, shown to the user
    /// before they choose.
    pub summary: String,
    /// Short style tags that seed the roleplay system prompt (e.g. "skeptical",
    /// "behavioral", "time-pressured").
    #[serde(default)]
    pub style_tags: Vec<String>,
    /// The AI's suggested pick for this meeting context.
    pub recommended: bool,
}

/// A web-research source folded into a [`KnowledgeProfile`] (Step 2), kept for
/// grounding and provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchSource {
    pub title: String,
    pub url: String,
    /// Short extracted snippet stored alongside the embedded chunk.
    pub snippet: String,
    pub fetched_at_unix_ms: u64,
}

/// The reusable, indexed knowledge base for a SimCon — attached library
/// documents + bounded web research, embedded into the RAG store. **Reusable**:
/// attach the same profile to a later SimCon or to a live call by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeProfile {
    pub id: String,
    pub title: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    /// `RagDocument` ids from the shared library this profile indexes (both
    /// user-attached files and Ally-generated context land in the library).
    #[serde(default)]
    pub doc_ids: Vec<String>,
    /// Bounded web-research results (Step 2).
    #[serde(default)]
    pub research: Vec<ResearchSource>,
    /// Whether ingest + indexing has completed and the profile is queryable.
    pub ready: bool,
}

/// One simulated-conversation record: Step 1 setup through Step 4 run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimConSession {
    pub id: String,
    /// e.g. "Senior Accountant Interview with CFO".
    pub title: String,
    /// The user's goal, e.g. "Prep for technical GAAP questions + leadership
    /// scenarios".
    pub purpose: String,
    /// For interviews (and similar roles), the target role's job description
    /// (Step 1). Grounds the counterparty's questions.
    #[serde(default)]
    pub job_description: Option<String>,
    pub category: SimConCategory,
    pub status: SimConStatus,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    /// Library documents attached at setup (Step 1, Path A) — `RagDocument` ids
    /// the ingestion phase folds into the `KnowledgeProfile`.
    #[serde(default)]
    pub source_doc_ids: Vec<String>,
    /// Whether Ally should auto-generate context (Step 1, Path B) during ingest.
    #[serde(default)]
    pub auto_generate_context: bool,
    /// The knowledge profile driving this session (reusable; referenced by id).
    #[serde(default)]
    pub knowledge_profile_id: Option<String>,
    /// The generated persona options (Step 3).
    #[serde(default)]
    pub personas: Vec<SimConPersona>,
    /// The persona the user chose to run against.
    #[serde(default)]
    pub chosen_persona_id: Option<String>,
    /// Once run, the resulting transcript is saved as a `Conversation` (by id).
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// The `RagDocument` id of the Ally-generated prep briefing, if one has been
    /// generated (also included in the profile's `doc_ids` so it grounds too).
    #[serde(default)]
    pub dossier_doc_id: Option<String>,
}

/// Catalog entry for the SimCon list view (cheap to list without loading the
/// full record + personas).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimConSummary {
    pub id: String,
    pub title: String,
    pub category: SimConCategory,
    pub status: SimConStatus,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl SimConCategory {
    /// Human phrase for prompts.
    pub fn label(self) -> &'static str {
        match self {
            SimConCategory::Interview => "job interview",
            SimConCategory::FinancialReview => "financial review",
            SimConCategory::PerformanceReview => "performance review",
            SimConCategory::SalesPitch => "sales pitch",
            SimConCategory::Other => "high-stakes conversation",
        }
    }
}

// ── Persona generation (Step 3) — pure prompt + parser ──────────────────────

/// Build the `(system, user)` prompt for generating 3 counterparty personas.
pub fn persona_prompt(session: &SimConSession) -> (String, String) {
    let system = "You generate realistic counterparty personas for rehearsing a \
high-stakes conversation. Return ONLY a JSON array of exactly 3 objects, each with \
keys: \"title\" (a short label), \"summary\" (2–3 sentences on how this person \
behaves in the room), \"style_tags\" (3–5 short lowercase strings), and \
\"recommended\" (boolean — set exactly one persona true, the best fit for this \
context). No prose, no markdown, no code fences."
        .to_string();

    let mut user = format!(
        "Rehearsal type: {}\nName: {}\nGoal: {}\n",
        session.category.label(),
        session.title,
        session.purpose,
    );
    if let Some(jd) = &session.job_description {
        if !jd.trim().is_empty() {
            user.push_str(&format!("Job description:\n{}\n", jd.trim()));
        }
    }
    user.push_str(
        "\nGenerate the 3 distinct counterparty personas the user should be ready \
to face, spanning different styles/difficulties.",
    );
    (system, user)
}

/// Parse the LLM's persona JSON into [`SimConPersona`]s, assigning ids. Tolerant:
/// extracts the first JSON array from the text (models sometimes wrap it in
/// prose/fences) and returns empty on malformed output. Ensures exactly one
/// persona is flagged recommended.
pub fn parse_personas(text: &str) -> Vec<SimConPersona> {
    #[derive(Deserialize)]
    struct Gen {
        title: String,
        #[serde(default)]
        summary: String,
        #[serde(default)]
        style_tags: Vec<String>,
        #[serde(default)]
        recommended: bool,
    }

    let slice = match (text.find('['), text.rfind(']')) {
        (Some(a), Some(b)) if b > a => &text[a..=b],
        _ => return Vec::new(),
    };
    let gens: Vec<Gen> = serde_json::from_str(slice).unwrap_or_default();

    let mut out: Vec<SimConPersona> = gens
        .into_iter()
        .filter(|g| !g.title.trim().is_empty())
        .enumerate()
        .map(|(i, g)| SimConPersona {
            id: format!("p{}", i + 1),
            title: g.title,
            summary: g.summary,
            style_tags: g.style_tags,
            recommended: g.recommended,
        })
        .collect();

    if !out.is_empty() && !out.iter().any(|p| p.recommended) {
        out[0].recommended = true;
    }
    out
}

// ── Live rehearsal (Step 4) — the counterparty's spoken turn ────────────────

/// Character budget for the transcript window fed to the persona (≈spoken
/// context; newest turns win when it bites).
const LIVE_TRANSCRIPT_CHAR_BUDGET: usize = 6_000;
/// Character budget for grounding (RAG chunks + research snippets).
const LIVE_REFERENCE_CHAR_BUDGET: usize = 4_000;

/// Build the LLM request for the AI counterparty's **next spoken turn** in a
/// live rehearsal. The model roleplays `persona`, grounded in the knowledge
/// profile (RAG `chunks` + `research`), replying to the conversation so far.
///
/// From the model's point of view it *is* the counterparty, so turns are
/// relabeled: the human's turns (outbound) are `User:` and the persona's own
/// prior turns (inbound) are `You:`. Output is spoken aloud, so the system
/// prompt asks for short, natural speech — no markdown, no stage directions.
pub fn persona_live_prompt(
    session: &SimConSession,
    persona: &SimConPersona,
    research: &[ResearchSource],
    segments: &[TranscriptSegment],
    chunks: &[ScoredChunk],
    max_tokens: u32,
) -> LlmRequest {
    let style = if persona.style_tags.is_empty() {
        String::new()
    } else {
        format!(" Your style: {}.", persona.style_tags.join(", "))
    };

    let mut system = format!(
        "You are roleplaying the user's counterparty in a {category} so they can \
rehearse. Stay fully in character as:\n{title} — {summary}{style}\n\n\
The user is the other person in the room. Speak ONLY as your character, in the \
first person, one turn at a time. This is spoken conversation: keep each reply \
short and natural (1–4 sentences), no markdown, no lists, no stage directions, \
no narration — just what your character says out loud. Stay realistic and \
specific using the background material. Never break character and never say you \
are an AI or that this is a simulation.",
        category = session.category.label(),
        title = persona.title,
        summary = persona.summary,
        style = style,
    );
    if let Some(jd) = session.job_description.as_deref() {
        let jd = jd.trim();
        if !jd.is_empty() {
            system.push_str(&format!(
                "\n\nThe role under discussion (for context you'd realistically \
know):\n{}",
                jd.chars().take(1_500).collect::<String>()
            ));
        }
    }

    // Grounding: RAG chunks first, then research snippets, under one budget.
    let mut reference = String::new();
    for chunk in chunks {
        let block = format!(
            "[source: {} — {}]\n{}\n\n",
            chunk.file_name, chunk.location, chunk.text
        );
        if reference.len() + block.len() > LIVE_REFERENCE_CHAR_BUDGET {
            break;
        }
        reference.push_str(&block);
    }
    for src in research {
        let block = format!("[web: {}]\n{}\n\n", src.title, src.snippet);
        if reference.len() + block.len() > LIVE_REFERENCE_CHAR_BUDGET {
            break;
        }
        reference.push_str(&block);
    }

    // Transcript window: newest-first until the budget is spent, then restore
    // order. Persona = inbound ("You:"), human = outbound ("User:").
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0usize;
    for segment in segments.iter().rev() {
        if !segment.is_final || segment.text.trim().is_empty() {
            continue;
        }
        let who = match segment.side {
            StreamSide::Inbound => "You",
            StreamSide::Outbound => "User",
        };
        let line = format!("{who}: {}\n", segment.text.trim());
        if used + line.len() > LIVE_TRANSCRIPT_CHAR_BUDGET {
            break;
        }
        used += line.len();
        lines.push(line);
    }
    lines.reverse();

    let mut user = String::new();
    if !reference.is_empty() {
        user.push_str("Background material:\n\n");
        user.push_str(&reference);
    }
    if lines.is_empty() {
        user.push_str(
            "The rehearsal is just starting. Open the conversation with your \
first line, in character.\n",
        );
    } else {
        user.push_str("Conversation so far:\n\n");
        user.push_str(&lines.concat());
        user.push_str(&format!(
            "\nIt's your turn. Reply as {}, in character.",
            persona.title
        ));
    }

    LlmRequest {
        system,
        user,
        max_tokens,
    }
}

// ── Prep dossier — Ally-authored briefing document ──────────────────────────

/// Reference budget for the dossier prompt (docs + research it synthesizes).
const DOSSIER_REFERENCE_CHAR_BUDGET: usize = 10_000;

/// Build the `(system, user)` prompt for the **Ally prep dossier**: a concise,
/// well-structured briefing Ally *writes* from the Sim Con's documents + web
/// research, saved back to the library as a readable document. Distinct from
/// retrieval — this is synthesis for the user (see
/// `conva_core/docs/technical/rag-and-ally-grounding.md`).
pub fn dossier_prompt(
    session: &SimConSession,
    research: &[ResearchSource],
    chunks: &[ScoredChunk],
    max_tokens: u32,
) -> LlmRequest {
    let system = format!(
        "You are Ally, preparing a concise pre-meeting briefing for the user before a {}. \
Write a well-structured prep document in Markdown with these sections: \
`## Overview` (2–3 sentences), `## Likely questions` (bulleted), `## Strong \
talking points` (bulleted), `## Background & key facts` (bulleted, specific), \
and `## Watch-outs` (bulleted). Ground everything in the provided material; be \
specific and useful, never generic. Do not invent facts or figures. Output only \
the Markdown document — no preamble.",
        session.category.label(),
    );

    let mut reference = String::new();
    for chunk in chunks {
        let block = format!("[{}]\n{}\n\n", chunk.file_name, chunk.text);
        if reference.len() + block.len() > DOSSIER_REFERENCE_CHAR_BUDGET {
            break;
        }
        reference.push_str(&block);
    }
    for src in research {
        let block = format!("[web: {}]\n{}\n\n", src.title, src.snippet);
        if reference.len() + block.len() > DOSSIER_REFERENCE_CHAR_BUDGET {
            break;
        }
        reference.push_str(&block);
    }

    let mut user = format!("Meeting: {}\nGoal: {}\n", session.title, session.purpose);
    if let Some(jd) = session.job_description.as_deref() {
        let jd = jd.trim();
        if !jd.is_empty() {
            user.push_str(&format!(
                "Role / job description:\n{}\n",
                jd.chars().take(2_000).collect::<String>()
            ));
        }
    }
    if reference.is_empty() {
        user.push_str(
            "\nNo documents or research were provided — write the briefing from \
what a well-prepared person should know for this meeting, and keep it clearly \
general.",
        );
    } else {
        user.push_str("\nMaterial to synthesize:\n\n");
        user.push_str(&reference);
    }

    LlmRequest {
        system,
        user,
        max_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_array_and_forces_one_recommended() {
        let text = r#"Here you go:
        [{"title":"Skeptical CFO","summary":"Tough.","style_tags":["skeptical"]},
         {"title":"Behavioral VP","summary":"Warm.","recommended":false}]"#;
        let p = parse_personas(text);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].id, "p1");
        assert!(
            p[0].recommended,
            "first is forced recommended when none set"
        );
    }

    #[test]
    fn parse_returns_empty_on_garbage() {
        assert!(parse_personas("no json here").is_empty());
    }

    fn sample_session() -> SimConSession {
        SimConSession {
            id: "s1".into(),
            title: "Senior Accountant Interview".into(),
            purpose: "Prep for GAAP questions".into(),
            job_description: Some("Own the monthly close.".into()),
            category: SimConCategory::Interview,
            status: SimConStatus::Ready,
            created_at_unix_ms: 0,
            updated_at_unix_ms: 0,
            source_doc_ids: vec![],
            auto_generate_context: false,
            knowledge_profile_id: None,
            personas: vec![],
            chosen_persona_id: None,
            conversation_id: None,
            dossier_doc_id: None,
        }
    }

    fn persona() -> SimConPersona {
        SimConPersona {
            id: "p1".into(),
            title: "Skeptical CFO".into(),
            summary: "Direct, numbers-first.".into(),
            style_tags: vec!["skeptical".into(), "technical".into()],
            recommended: true,
        }
    }

    fn seg(side: StreamSide, seq: u64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            side,
            seq,
            text: text.to_string(),
            is_final: true,
            start_ms: seq * 1000,
            end_ms: seq * 1000 + 900,
            confidence: None,
            latency_ms: 100,
        }
    }

    #[test]
    fn live_prompt_roleplays_persona_and_relabels_turns() {
        let segments = vec![
            seg(StreamSide::Outbound, 0, "Hi, thanks for meeting me."),
            seg(StreamSide::Inbound, 1, "Let's get into the numbers."),
            seg(StreamSide::Outbound, 2, "Sure, ask away."),
        ];
        let req = persona_live_prompt(&sample_session(), &persona(), &[], &segments, &[], 300);

        assert!(req.system.contains("Skeptical CFO"));
        assert!(req.system.contains("job interview"));
        assert!(req.system.contains("Own the monthly close."), "JD included");
        // Human = User:, persona = You:
        assert!(req.user.contains("User: Hi, thanks for meeting me."));
        assert!(req.user.contains("You: Let's get into the numbers."));
        assert!(req.user.contains("It's your turn"));
    }

    #[test]
    fn live_prompt_opens_when_no_turns_yet() {
        let req = persona_live_prompt(&sample_session(), &persona(), &[], &[], &[], 300);
        assert!(req.user.contains("Open the conversation"));
    }

    #[test]
    fn dossier_prompt_has_sections_and_synthesizes_material() {
        let chunks = vec![ScoredChunk {
            document_id: "d1".into(),
            file_name: "resume.pdf".into(),
            location: "p1".into(),
            text: "Led the monthly close for 3 years.".into(),
            score: 0.9,
        }];
        let req = dossier_prompt(&sample_session(), &[], &chunks, 1200);
        assert!(req.system.contains("Likely questions"));
        assert!(req.system.contains("job interview"));
        assert!(req.user.contains("Led the monthly close"));
    }
}
