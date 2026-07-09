/**
 * Typed wrappers around the shell's Tauri commands (src-tauri/src/lib.rs).
 * In a plain browser tab these reject; callers guard with isTauri().
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  AppConfig,
  AssistKind,
  AudioDevice,
  ModelInfo,
  ProviderId,
  ProviderInfo,
  ProviderKeyStatus,
  TranscriptSegment,
} from "@/lib/ipc";

export function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

export function saveConfig(config: AppConfig): Promise<void> {
  return invoke("save_config", { config });
}

export function getProviderRegistry(): Promise<ProviderInfo[]> {
  return invoke<ProviderInfo[]>("get_provider_registry");
}

export function listAudioDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_audio_devices");
}

export function startSession(): Promise<string> {
  return invoke<string>("start_session");
}

export function stopSession(): Promise<void> {
  return invoke("stop_session");
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
