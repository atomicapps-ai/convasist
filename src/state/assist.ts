import { create } from "zustand";

import { assist as invokeAssist } from "@/lib/commands";
import type { AssistChunkEvent, AssistKind } from "@/lib/ipc";
import { useTranscriptStore } from "@/state/transcript";

export interface AssistCard {
  id: string;
  kind: AssistKind;
  question: string | null;
  text: string;
  done: boolean;
  error: string | null;
  startedAtMs: number;
}

interface AssistState {
  cards: AssistCard[];
  busy: boolean;

  request: (kind: AssistKind, question?: string) => Promise<void>;
  applyChunk: (chunk: AssistChunkEvent) => void;
  clear: () => void;
}

let counter = 0;

export const useAssistStore = create<AssistState>((set, get) => ({
  cards: [],
  busy: false,

  request: async (kind, question) => {
    if (get().busy) return;
    counter += 1;
    const id = `assist-${Date.now()}-${counter}`;
    set((s) => ({
      busy: true,
      // Keep the last few cards; newest first.
      cards: [
        {
          id,
          kind,
          question: question ?? null,
          text: "",
          done: false,
          error: null,
          startedAtMs: Date.now(),
        },
        ...s.cards.slice(0, 4),
      ],
    }));
    try {
      const segments = useTranscriptStore.getState().segments;
      await invokeAssist(id, kind, question ?? null, segments);
    } catch (e) {
      set((s) => ({
        busy: false,
        cards: s.cards.map((c) =>
          c.id === id ? { ...c, done: true, error: String(e) } : c,
        ),
      }));
    }
  },

  applyChunk: (chunk) =>
    set((s) => ({
      busy: chunk.done ? false : s.busy,
      cards: s.cards.map((c) =>
        c.id === chunk.request_id
          ? {
              ...c,
              text: c.text + chunk.token,
              done: chunk.done,
              error: chunk.error,
            }
          : c,
      ),
    })),

  clear: () => set({ cards: [] }),
}));
