/**
 * Typed wrappers around the shell's Tauri commands (src-tauri/src/lib.rs).
 * In a plain browser tab these reject; callers guard with isTauri().
 */

import { invoke } from "@tauri-apps/api/core";

import type { AppConfig, AudioDevice, ProviderInfo } from "@/lib/ipc";

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
