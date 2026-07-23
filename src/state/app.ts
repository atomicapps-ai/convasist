import { create } from "zustand";

import {
  getConfig,
  getProviderRegistry,
  listAudioDevices,
  providerKeyStatus,
  recordingStatus,
  saveConfig,
  startRecording,
  startSession,
  stopRecording,
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
  /** Sidecar mode (U9): narrow always-on-top strip beside a call window. */
  sidecar: boolean;
  toggleSidecar: () => Promise<void>;
  /** Call recording (stereo WAV: you = left, them = right). */
  recording: boolean;
  recordingPath: string | null;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<void>;

  init: () => Promise<void>;
  /** Re-enumerate audio devices (e.g. after plugging one in). */
  refreshDevices: () => Promise<void>;
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
  recording: false,
  recordingPath: null,
  startRecording: async () => {
    try {
      const path = await startRecording();
      set({ recording: true, recordingPath: path });
    } catch (e) {
      set({ lastError: String(e) });
    }
  },
  stopRecording: async () => {
    try {
      const path = await stopRecording();
      set({ recording: false, recordingPath: path });
    } catch (e) {
      set({ recording: false, lastError: String(e) });
    }
  },
  sidecar: false,
  toggleSidecar: async () => {
    const next = !get().sidecar;
    set({ sidecar: next });
    try {
      const { applySidecar } = await import("@/lib/sidecar");
      await applySidecar(next);
    } catch (e) {
      set({ sidecar: !next, lastError: String(e) });
    }
  },
  refreshKeyStatus: async () => {
    const statuses = await providerKeyStatus();
    set({
      keyStatus: Object.fromEntries(statuses.map((s) => [s.id, s.has_key])),
    });
  },

  init: async () => {
    if (!isTauri()) return;
    try {
      const [config, devices, registry, keys, recording] = await Promise.all([
        getConfig(),
        listAudioDevices(),
        getProviderRegistry(),
        providerKeyStatus(),
        recordingStatus(),
      ]);
      set({
        config,
        devices,
        registry,
        keyStatus: Object.fromEntries(keys.map((s) => [s.id, s.has_key])),
        recording,
      });
    } catch (e) {
      set({ lastError: String(e) });
    }
  },

  refreshDevices: async () => {
    if (!isTauri()) return;
    try {
      set({ devices: await listAudioDevices() });
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
      // Offer to save the conversation when anything was transcribed
      // (owner flow: Stop → "save this conversation?").
      const [{ useTranscriptStore }, { useConversationStore }] =
        await Promise.all([
          import("@/state/transcript"),
          import("@/state/conversation"),
        ]);
      const t = useTranscriptStore.getState();
      const hasContent =
        t.archived.length > 0 ||
        t.segments.some((s) => s.is_final && s.text.trim().length > 0);
      if (hasContent) {
        useConversationStore.getState().setSavePromptOpen(true);
      }
    } catch (e) {
      set({ lastError: String(e) });
    } finally {
      // The session stop finalizes any recording backend-side.
      set({ busy: false, recording: false });
    }
  },
}));
