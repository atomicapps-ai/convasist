import { create } from "zustand";

import { assist as invokeAssist } from "@/lib/commands";
import type {
  AssistChunkEvent,
  AssistKind,
  AssistSource,
  AssistSourcesEvent,
  RadarEvent,
  TrackerEvent,
} from "@/lib/ipc";
import { useTranscriptStore } from "@/state/transcript";

export interface AssistCard {
  id: string;
  kind: AssistKind;
  question: string | null;
  text: string;
  done: boolean;
  error: string | null;
  sources: AssistSource[];
  startedAtMs: number;
  /** Transcript bubble this answer researches (`"<side>-<seq>"`), if any —
   *  drives the connector line from the AI column back to the bubble. */
  sourceKey: string | null;
  /** Short quote of the researched bubble, shown on the card. */
  sourceQuote: string | null;
}

interface AssistState {
  cards: AssistCard[];
  busy: boolean;
  /** Latest Question Radar hit (§6.2); replaced by each new question. */
  radar: RadarEvent | null;
  /** Cumulative session tracker state (§6.3). */
  tracker: TrackerEvent | null;

  request: (
    kind: AssistKind,
    question?: string,
    source?: { key: string; quote: string },
  ) => Promise<void>;
  applyChunk: (chunk: AssistChunkEvent) => void;
  applySources: (event: AssistSourcesEvent) => void;
  applyRadar: (event: RadarEvent) => void;
  applyTracker: (event: TrackerEvent) => void;
  dismissRadar: () => void;
  clear: () => void;
}

let counter = 0;

export const useAssistStore = create<AssistState>((set, get) => ({
  cards: [],
  busy: false,
  radar: null,
  tracker: null,

  request: async (kind, question, source) => {
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
          sources: [],
          startedAtMs: Date.now(),
          sourceKey: source?.key ?? null,
          sourceQuote: source?.quote ?? null,
        },
        ...s.cards.slice(0, 5),
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

  applySources: (event) =>
    set((s) => ({
      cards: s.cards.map((c) =>
        c.id === event.request_id ? { ...c, sources: event.sources } : c,
      ),
    })),

  applyRadar: (event) => set({ radar: event }),

  applyTracker: (event) => set({ tracker: event }),

  dismissRadar: () => set({ radar: null }),

  clear: () => set({ cards: [], radar: null, tracker: null }),
}));
