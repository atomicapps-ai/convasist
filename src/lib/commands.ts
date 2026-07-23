/**
 * Typed wrappers around the shell's Tauri commands (src-tauri/src/lib.rs).
 * In a plain browser tab these reject; callers guard with isTauri().
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  AppConfig,
  AssistKind,
  AudioDevice,
  Conversation,
  ConversationSummary,
  IngestReport,
  ModelInfo,
  ProviderId,
  ProviderInfo,
  ProviderKeyStatus,
  RagDocument,
  SecretsStatus,
  SessionSummary,
  TranscriptSegment,
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

export function assist(
  requestId: string,
  kind: AssistKind,
  question: string | null,
  segments: TranscriptSegment[],
): Promise<void> {
  return invoke("assist", { requestId, kind, question, segments });
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
