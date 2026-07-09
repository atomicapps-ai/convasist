import { create } from "zustand";

import {
  getConfig,
  listAudioDevices,
  saveConfig,
  startSession,
  stopSession,
} from "@/lib/commands";
import { isTauri, type AppConfig, type AudioDevice } from "@/lib/ipc";

interface AppState {
  config: AppConfig | null;
  devices: AudioDevice[];
  busy: boolean;
  lastError: string | null;

  init: () => Promise<void>;
  updateConfig: (patch: Partial<AppConfig>) => Promise<void>;
  acknowledgeConsent: () => Promise<void>;
  start: () => Promise<void>;
  stop: () => Promise<void>;
}

export const useAppStore = create<AppState>((set, get) => ({
  config: null,
  devices: [],
  busy: false,
  lastError: null,

  init: async () => {
    if (!isTauri()) return;
    try {
      const [config, devices] = await Promise.all([
        getConfig(),
        listAudioDevices(),
      ]);
      set({ config, devices });
    } catch (e) {
      set({ lastError: String(e) });
    }
  },

  updateConfig: async (patch) => {
    const current = get().config;
    if (!current) return;
    const next = { ...current, ...patch };
    set({ config: next });
    try {
      await saveConfig(next);
    } catch (e) {
      set({ config: current, lastError: String(e) });
    }
  },

  acknowledgeConsent: async () => {
    await get().updateConfig({ consent_acknowledged: true });
  },

  start: async () => {
    set({ busy: true, lastError: null });
    try {
      await startSession();
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      set({ busy: false });
    }
  },

  stop: async () => {
    set({ busy: true });
    try {
      await stopSession();
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      set({ busy: false });
    }
  },
}));
