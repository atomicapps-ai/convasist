import { create } from "zustand";

import type {
  AudioLevelEvent,
  SessionStateEvent,
  StreamSide,
  TranscriptSegment,
} from "@/lib/ipc";

interface TranscriptState {
  /** Chronological finalized + in-flight segments, both sides interleaved. */
  segments: TranscriptSegment[];
  session: SessionStateEvent;
  levels: Record<StreamSide, AudioLevelEvent | null>;

  applySegment: (segment: TranscriptSegment) => void;
  setSession: (session: SessionStateEvent) => void;
  setLevel: (level: AudioLevelEvent) => void;
  clear: () => void;
}

/**
 * A final segment replaces every partial that carried the same (side, seq);
 * a newer partial replaces the previous partial for its (side, seq).
 */
export const useTranscriptStore = create<TranscriptState>((set) => ({
  segments: [],
  session: { state: "idle" },
  levels: { inbound: null, outbound: null },

  applySegment: (segment) =>
    set((s) => {
      const idx = s.segments.findIndex(
        (existing) =>
          existing.side === segment.side && existing.seq === segment.seq,
      );
      if (idx === -1) {
        return { segments: [...s.segments, segment] };
      }
      const next = s.segments.slice();
      next[idx] = segment;
      return { segments: next };
    }),

  setSession: (session) => set({ session }),

  setLevel: (level) =>
    set((s) => ({ levels: { ...s.levels, [level.side]: level } })),

  clear: () => set({ segments: [] }),
}));
