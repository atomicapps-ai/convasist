import { create } from "zustand";

import {
  getConfig,
  getProviderRegistry,
  listAudioDevices,
  providerKeyStatus,
  saveConfig,
  startSession,
  stopSession,
} from "@/lib/commands";
import {
  isTauri,
  type AppConfig,
  type AudioDevice,
  type ModelStatusEvent,
  type ProviderId,
  type ProviderInfo,
} from "@/lib/ipc";

interface AppState {
  config: AppConfig | null;
  devices: AudioDevice[];
  busy: boolean;
  lastError: string | null;
  modelStatus: ModelStatusEvent | null;
  setModelStatus: (status: ModelStatusEvent) => void;
  registry: ProviderInfo[];
  keyStatus: Partial<Record<ProviderId, boolean>>;
  refreshKeyStatus: () => Promise<void>;

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
  modelStatus: null,
  setModelStatus: (status) => {
    // A finished download clears the "model_downloading" start error.
    set((s) => ({
      modelStatus: status,
      lastError:
        status.state === "ready" &&
        s.lastError?.includes("model_downloading")
          ? null
          : s.lastError,
    }));
  },

  registry: [],
  keyStatus: {},
  refreshKeyStatus: async () => {
    const statuses = await providerKeyStatus();
    set({
      keyStatus: Object.fromEntries(statuses.map((s) => [s.id, s.has_key])),
    });
  },

  init: async () => {
    if (!isTauri()) return;
    try {
      const [config, devices, registry, keys] = await Promise.all([
        getConfig(),
        listAudioDevices(),
        getProviderRegistry(),
        providerKeyStatus(),
      ]);
      set({
        config,
        devices,
        registry,
        keyStatus: Object.fromEntries(keys.map((s) => [s.id, s.has_key])),
      });
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
