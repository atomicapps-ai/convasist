/**
 * Typed mirror of the Rust IPC contract.
 *
 * Source of truth: crates/conva-core/src/ipc.rs — if that file changes,
 * this one changes in the same commit (ts-rs codegen replaces this hand
 * mirror later in Phase 1).
 */

export type StreamSide = "inbound" | "outbound";

export const EVENTS = {
  transcriptSegment: "conva://transcript-segment",
  audioLevel: "conva://audio-level",
  sessionState: "conva://session-state",
  allyChunk: "conva://ally-chunk",
  modelStatus: "conva://model-status",
  allySources: "conva://ally-sources",
  radar: "conva://radar",
  tracker: "conva://tracker",
  authChanged: "conva://auth-changed",
} as const;

export interface TranscriptSegment {
  side: StreamSide;
  seq: number;
  text: string;
  is_final: boolean;
  start_ms: number;
  end_ms: number;
  confidence: number | null;
  latency_ms: number;
}

export interface AudioLevelEvent {
  side: StreamSide;
  rms_dbfs: number;
  healthy: boolean;
}

export type SessionStateEvent =
  | { state: "idle" }
  /** Start underway: model loading / first-run GPU shader compile. */
  | { state: "preparing"; message: string }
  | { state: "listening"; session_id: string; started_at_unix_ms: number }
  | { state: "paused"; session_id: string }
  | { state: "error"; message: string };

/** Live Sim Con rehearsal phase — drives the speaking/active-speaker UI. */
export type RehearsalStateEvent =
  | { phase: "listening" }
  | { phase: "thinking" }
  | { phase: "speaking" }
  | { phase: "ended" };

export interface AllyChunkEvent {
  request_id: string;
  token: string;
  done: boolean;
  error: string | null;
}

/** Mirror of conva-core prompt::AllyKind. */
export type AllyKind = "suggest_reply" | "summarize" | "question";

export interface ModelInfo {
  id: string;
  display_name: string;
}

/** Mirror of the shell's WhisperModelInfo (speech-to-text model picker). */
export interface WhisperModelInfo {
  id: string;
  label: string;
  note: string;
  approx_mb: number;
}

export interface ProviderKeyStatus {
  id: ProviderId;
  has_key: boolean;
}

export interface AllySource {
  file_name: string;
  location: string;
}

export interface AllySourcesEvent {
  request_id: string;
  sources: AllySource[];
}

/** Mirror of conva-core rag::RagDocument. */
export interface RagDocument {
  id: string;
  file_name: string;
  enabled: boolean;
  chunk_count: number;
  ingested_at_unix_ms: number;
}

export interface IngestReport {
  document: RagDocument;
  warnings: string[];
}

/** Mirror of the shell's SecretsStatus (portable encrypted secrets). */
export interface SecretsStatus {
  passphrase_set: boolean;
  file_present: boolean;
  file_path: string;
  passphrase_env: string;
}

/** Mirror of the shell's AuthStatus (account sign-in via Supabase OAuth). */
export interface AuthStatus {
  signed_in: boolean;
  email: string | null;
  user_id: string | null;
  expires_at_unix: number | null;
  /** False when no Supabase anon key is configured — sign-in unavailable. */
  configured: boolean;
}

/** Mirror of the shell's AuthChangedEvent: an OAuth sign-in finishing
 *  out-of-band via the conva://auth/callback deep link. Exactly one of
 *  `status` / `error` is set. */
export interface AuthChangedEvent {
  status: AuthStatus | null;
  error: string | null;
}

export interface ScoredChunk {
  document_id: string;
  file_name: string;
  location: string;
  text: string;
  score: number;
}

export interface RadarEvent {
  question: string;
  sources: ScoredChunk[];
}

export interface TrackedEntity {
  label: string;
  detail: string;
}

export interface TrackedCommitment {
  who: string; // "you" | "them"
  what: string;
  due: string;
}

export interface TrackerEvent {
  entities: TrackedEntity[];
  commitments: TrackedCommitment[];
}

export interface SessionSummary {
  id: string;
  started_at_unix_ms: number;
  segment_count: number;
  preview: string;
  /** True when this session was a Sim Con rehearsal. */
  is_rehearsal: boolean;
  /** The Sim Con title, when this was a rehearsal. */
  simcon_title: string | null;
}

/** Mirror of the shell's conversations::Conversation (named saved record). */
export interface Conversation {
  id: string;
  title: string;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
  segments: TranscriptSegment[];
  linked_docs: string[];
}

/** Mirror of the shell's conversations::ConversationSummary. */
export interface ConversationSummary {
  id: string;
  title: string;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
  segment_count: number;
  linked_docs: string[];
  preview: string;
}

/* ── SimCon — Simulated Conversation (mirror of conva_core::simcon) ──────────
   A rehearsal of a high-stakes call: setup → knowledge profile (docs + bounded
   web research) → generated personas → real-time run. Persistence + pipeline
   land in the shell (Phase A.2). Keep these in lockstep with
   `crates/conva-core/src/simcon.rs`. */

/** The kind of call being rehearsed. */
export type SimConCategory =
  | "interview"
  | "financial_review"
  | "performance_review"
  | "sales_pitch"
  | "other";

/** Lifecycle of a SimCon, start to finish. */
export type SimConStatus =
  | "draft"
  | "ingesting"
  | "ready"
  | "running"
  | "ended";

/** One generated counterparty persona/strategy option (3 per session). */
export interface SimConPersona {
  id: string;
  title: string;
  summary: string;
  style_tags: string[];
  recommended: boolean;
}

/** A web-research source folded into a knowledge profile. */
export interface ResearchSource {
  title: string;
  url: string;
  snippet: string;
  fetched_at_unix_ms: number;
}

/** The reusable, indexed knowledge base for a SimCon (library docs + web
 *  research). Reusable across future SimCons and live calls, by id. */
export interface KnowledgeProfile {
  id: string;
  title: string;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
  doc_ids: string[];
  research: ResearchSource[];
  ready: boolean;
}

/** One simulated-conversation record: Step 1 setup through Step 4 run. */
export interface SimConSession {
  id: string;
  title: string;
  purpose: string;
  /** For interviews: the target role's job description (Step 1). */
  job_description: string | null;
  category: SimConCategory;
  status: SimConStatus;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
  /** Library docs attached at setup (Path A) — RagDocument ids. */
  source_doc_ids: string[];
  /** Whether Ally should auto-generate context (Path B) during ingest. */
  auto_generate_context: boolean;
  knowledge_profile_id: string | null;
  personas: SimConPersona[];
  chosen_persona_id: string | null;
  conversation_id: string | null;
  /** RagDocument id of the Ally-generated prep briefing, once generated. */
  dossier_doc_id: string | null;
}

/** Catalog entry for the SimCon list view. */
export interface SimConSummary {
  id: string;
  title: string;
  category: SimConCategory;
  status: SimConStatus;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
}

export type ModelStatusEvent =
  | { state: "downloading"; model: string; percent: number }
  | { state: "ready"; model: string }
  | { state: "error"; model: string; message: string };

/** Mirror of conva-core llm::ProviderId (snake_case serde). */
export type ProviderId =
  | "anthropic"
  | "openai"
  | "google"
  | "xai"
  | "deepseek"
  | "ollama_local";

export interface ProviderInfo {
  id: ProviderId;
  name: string;
  default_quality_model: string;
  default_fast_model: string;
  requires_api_key: boolean;
  is_local: boolean;
}

export interface ModelSelection {
  provider: ProviderId;
  model: string;
}

/* ── Usage metering (mirror of conva_core::metering) ────────────────────────
   LLM tokens per provider + Tavily search count, for Settings → Usage. On the
   desktop this is BYO-key visibility; the hosted future turns it into billable
   credits (roadmap F8b). */

/** Running LLM usage for one provider. */
export interface ProviderUsage {
  provider: ProviderId;
  input_tokens: number;
  output_tokens: number;
  requests: number;
}

/** Usage snapshot with cross-provider running totals. */
export interface UsageSummary {
  providers: ProviderUsage[];
  total_input_tokens: number;
  total_output_tokens: number;
  total_requests: number;
  /** Tavily searches (Tavily bills per search, not per token). */
  tavily_searches: number;
  /** TTS characters synthesized (Aura bills per character). */
  tts_characters: number;
  /** When the current window opened (first record / last reset); 0 = never. */
  since_unix_ms: number;
  updated_at_unix_ms: number;
}

export interface AppConfig {
  asr_engine: "whisper_local" | "deepgram_cloud";
  whisper_model: string;
  llm_quality: ModelSelection;
  llm_fast: ModelSelection | null;
  consent_acknowledged: boolean;
  input_device: string | null;
  loopback_device: string | null;
  tracker_enabled: boolean;
  vad_neural: boolean;
  vad_sensitivity: number;
}

/** Mirror of conva-core audio::AudioDevice. */
export interface AudioDevice {
  id: string;
  name: string;
  side: StreamSide;
  is_default: boolean;
}

/** True when running inside the Tauri shell (vs a plain browser dev tab). */
export function isTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}
