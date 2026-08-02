/**
 * Typed wrappers around the shell's Tauri commands (src-tauri/src/lib.rs).
 * In a plain browser tab these reject; callers guard with isTauri().
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  AppConfig,
  AllyKind,
  AudioDevice,
  AuthStatus,
  Conversation,
  ConversationSummary,
  KnowledgeProfile,
  SimConSession,
  SimConSummary,
  IngestReport,
  ModelInfo,
  ProviderId,
  ProviderInfo,
  ProviderKeyStatus,
  RagDocument,
  SecretsStatus,
  SessionSummary,
  TranscriptSegment,
  UsageSummary,
  WhisperModelInfo,
} from "@/lib/ipc";

export function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

export function saveConfig(config: AppConfig): Promise<void> {
  return invoke("save_config", { config });
}

/** Write the current config to a JSON file (for committing to the repo). */
export function exportConfig(path: string): Promise<void> {
  return invoke("export_config", { path });
}

/** Load a config file, apply it live, and persist it. */
export function importConfig(path: string): Promise<AppConfig> {
  return invoke<AppConfig>("import_config", { path });
}

export function getProviderRegistry(): Promise<ProviderInfo[]> {
  return invoke<ProviderInfo[]>("get_provider_registry");
}

export function listAudioDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_audio_devices");
}

export function listWhisperModels(): Promise<WhisperModelInfo[]> {
  return invoke<WhisperModelInfo[]>("list_whisper_models");
}

/** Store (empty string clears) the Deepgram API key in the OS vault. */
export function setDeepgramKey(key: string): Promise<void> {
  return invoke("set_deepgram_key", { key });
}

export function deepgramKeyStatus(): Promise<boolean> {
  return invoke<boolean>("deepgram_key_status");
}

export function startSession(): Promise<string> {
  return invoke<string>("start_session");
}

export function stopSession(): Promise<void> {
  return invoke("stop_session");
}

/** Start recording the live call to a stereo WAV; resolves to the file path. */
export function startRecording(): Promise<string> {
  return invoke<string>("start_recording");
}

/** Stop recording; resolves to the saved file path (null if none was active). */
export function stopRecording(): Promise<string | null> {
  return invoke<string | null>("stop_recording");
}

export function recordingStatus(): Promise<boolean> {
  return invoke<boolean>("recording_status");
}

export function setApiKey(provider: ProviderId, key: string): Promise<void> {
  return invoke("set_api_key", { provider, key });
}

export function providerKeyStatus(): Promise<ProviderKeyStatus[]> {
  return invoke<ProviderKeyStatus[]>("provider_key_status");
}

/** Returns measured first-token latency in ms. */
export function testProvider(
  provider: ProviderId,
  model: string,
): Promise<number> {
  return invoke<number>("test_provider", { provider, model });
}

export function listProviderModels(
  provider: ProviderId,
): Promise<ModelInfo[]> {
  return invoke<ModelInfo[]>("list_provider_models", { provider });
}

export function ally(
  requestId: string,
  kind: AllyKind,
  question: string | null,
  segments: TranscriptSegment[],
): Promise<void> {
  return invoke("ally", { requestId, kind, question, segments });
}

export function ragIngest(paths: string[]): Promise<IngestReport[]> {
  return invoke<IngestReport[]>("rag_ingest", { paths });
}

/** Ingest clipboard/pasted text as a `.txt` document in the library. */
export function ragIngestText(
  name: string,
  text: string,
): Promise<IngestReport> {
  return invoke<IngestReport>("rag_ingest_text", { name, text });
}

export function ragList(): Promise<RagDocument[]> {
  return invoke<RagDocument[]>("rag_list");
}

export function ragSetEnabled(id: string, enabled: boolean): Promise<void> {
  return invoke("rag_set_enabled", { id, enabled });
}

export function ragDelete(id: string): Promise<void> {
  return invoke("rag_delete", { id });
}

/** Download a document back to `dest` (original file, or reconstructed text). */
export function ragDownload(id: string, dest: string): Promise<void> {
  return invoke("rag_download", { id, dest });
}

export function secretsStatus(): Promise<SecretsStatus> {
  return invoke<SecretsStatus>("secrets_status");
}

/** Encrypt stored keys to `dest` (or the default path) for committing to git. */
export function secretsExport(dest?: string): Promise<string> {
  return invoke<string>("secrets_export", { dest: dest ?? null });
}

/** Decrypt a secrets file and load its keys into the OS vault. */
export function secretsImport(
  src?: string,
  overwrite = false,
): Promise<string> {
  return invoke<string>("secrets_import", { src: src ?? null, overwrite });
}

/** Begin OAuth sign-in: opens the system browser and resolves immediately —
 *  the outcome arrives as an EVENTS.authChanged event when the browser
 *  deep-links back into the app (conva://auth/callback). */
export function authStart(provider?: string): Promise<void> {
  return invoke("auth_start", { provider: provider ?? null });
}

/** Abandon a pending OAuth sign-in (stops the "waiting for browser" state);
 *  a deep link arriving afterwards is ignored as stale. */
export function authCancel(): Promise<void> {
  return invoke("auth_cancel");
}

/** Open an external URL in the system browser (e.g. the website's
 *  password-reset page). */
export function openUrl(url: string): Promise<void> {
  return invoke("open_url", { url });
}

/** RAG-grounded relevant phrases in a transcript message, for highlighting. */
export function analyzeTerms(text: string): Promise<string[]> {
  return invoke<string[]>("analyze_terms", { text });
}

/** Sign in with email + password (same account as Google / the website). */
export function authSigninPassword(
  email: string,
  password: string,
): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_signin_password", { email, password });
}

/** Create an account with email + password. Rejects with
 *  `email_confirmation_required` when the email must be confirmed first. */
export function authSignupPassword(
  email: string,
  password: string,
): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_signup_password", { email, password });
}

/** Current signed-in snapshot ("signed in as…"), read offline. */
export function authStatus(): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_status");
}

/** Sign out: revoke server-side (best-effort) and clear local tokens. */
export function authSignout(): Promise<void> {
  return invoke("auth_signout");
}

/** Write a diagnostics report to a log file; resolves to the saved path. */
export function saveDebugLog(contents: string): Promise<string> {
  return invoke<string>("save_debug_log", { contents });
}

/**
 * Create or update a named conversation. Passing an existing `id` replaces
 * the stored record with this (fuller) transcript — append semantics.
 */
export function conversationSave(
  id: string | null,
  title: string | null,
  segments: TranscriptSegment[],
  linkedDocs: string[],
): Promise<Conversation> {
  return invoke<Conversation>("conversation_save", {
    id,
    title,
    segments,
    linkedDocs,
  });
}

export function conversationList(): Promise<ConversationSummary[]> {
  return invoke<ConversationSummary[]>("conversation_list");
}

export function conversationLoad(id: string): Promise<Conversation> {
  return invoke<Conversation>("conversation_load", { id });
}

export function conversationDelete(id: string): Promise<void> {
  return invoke("conversation_delete", { id });
}

/* ── SimCon (Simulated Conversation) ── */

/** Create or update a SimCon. An empty `id` mints a new record. */
export function simconSave(session: SimConSession): Promise<SimConSession> {
  return invoke<SimConSession>("simcon_save", { session });
}

export function simconList(): Promise<SimConSummary[]> {
  return invoke<SimConSummary[]>("simcon_list");
}

export function simconLoad(id: string): Promise<SimConSession> {
  return invoke<SimConSession>("simcon_load", { id });
}

export function simconDelete(id: string): Promise<void> {
  return invoke("simcon_delete", { id });
}

/** Copy documents into a Sim Con's folder (named after its title); returns the
 *  new in-folder paths to ingest into the RAG library. */
export function simconStoreDocs(
  title: string,
  paths: string[],
): Promise<string[]> {
  return invoke<string[]>("simcon_store_docs", { title, paths });
}

/** Build the reusable KnowledgeProfile (docs + research) and mark the Sim Con
 *  ready; returns the updated session. */
export function simconPrepare(id: string): Promise<SimConSession> {
  return invoke<SimConSession>("simcon_prepare", { id });
}

/** Load a Sim Con's knowledge base (attached docs + researched sources). */
export function simconLoadProfile(profileId: string): Promise<KnowledgeProfile> {
  return invoke<KnowledgeProfile>("simcon_load_profile", { profileId });
}

/** Generate the Ally prep dossier (saved to the library); returns the session. */
export function simconGenerateDossier(id: string): Promise<SimConSession> {
  return invoke<SimConSession>("simcon_generate_dossier", { id });
}

/** Reconstruct a library document's text (e.g. to show the prep dossier). */
export function ragDocumentText(id: string): Promise<string | null> {
  return invoke<string | null>("rag_document_text", { id });
}

/** Generate 3 counterparty personas with the configured LLM. */
export function simconGeneratePersonas(id: string): Promise<SimConSession> {
  return invoke<SimConSession>("simcon_generate_personas", { id });
}

/** Record the chosen persona. */
export function simconChoosePersona(
  id: string,
  personaId: string,
): Promise<SimConSession> {
  return invoke<SimConSession>("simcon_choose_persona", { id, personaId });
}

/** Start a live rehearsal (mic → persona LLM → Aura TTS). Returns session id. */
export function simconStartRehearsal(id: string): Promise<string> {
  return invoke<string>("simcon_start_rehearsal", { id });
}

/** End the user's current rehearsal turn now (manual "your turn"). */
export function simconRehearsalYourTurn(): Promise<void> {
  return invoke("simcon_rehearsal_your_turn");
}

/** Inject a typed turn (e.g. an Ally-suggested answer) as the user's turn. */
export function simconRehearsalSay(text: string): Promise<void> {
  return invoke("simcon_rehearsal_say", { text });
}

/** Store (empty clears) the Tavily web-research key in the OS vault. */
export function setTavilyKey(key: string): Promise<void> {
  return invoke("set_tavily_key", { key });
}

export function tavilyKeyStatus(): Promise<boolean> {
  return invoke<boolean>("tavily_key_status");
}

/** Usage snapshot (LLM tokens per provider + Tavily searches) for Settings. */
export function usageSummary(): Promise<UsageSummary> {
  return invoke<UsageSummary>("usage_summary");
}

/** Clear all usage counters; returns the emptied snapshot. */
export function usageReset(): Promise<UsageSummary> {
  return invoke<UsageSummary>("usage_reset");
}

/** Copy library originals into the repo `library/` folder for git commit. */
export function ragSyncLibrary(): Promise<string> {
  return invoke<string>("rag_sync_library");
}

export function sessionList(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("session_list");
}

export function sessionLoad(id: string): Promise<TranscriptSegment[]> {
  return invoke<TranscriptSegment[]>("session_load", { id });
}

export function exportTranscript(
  path: string,
  segments: TranscriptSegment[],
): Promise<void> {
  return invoke("export_transcript", { path, segments });
}

// --- Floating HUD panel (src-tauri/src/hud.rs) ------------------------------

/** Open the floating HUD panel (or re-pin it if already open). */
export function openHud(): Promise<void> {
  return invoke("open_hud");
}

/** Close and destroy the floating HUD panel. */
export function closeHud(): Promise<void> {
  return invoke("close_hud");
}

/** Toggle the floating HUD panel. Resolves to the new state (true = open). */
export function toggleHud(): Promise<boolean> {
  return invoke<boolean>("toggle_hud");
}

/** Whether the floating HUD panel is currently open. */
export function hudIsOpen(): Promise<boolean> {
  return invoke<boolean>("hud_is_open");
}
