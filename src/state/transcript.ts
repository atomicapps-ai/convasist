import { create } from "zustand";

import type {
  AudioLevelEvent,
  SessionStateEvent,
  StreamSide,
  TranscriptSegment,
} from "@/lib/ipc";

/**
 * Archived (earlier-run / loaded-conversation) segments are re-numbered into
 * this seq range so their (side, seq) identity can never collide with the
 * live session, whose engine seqs restart at 0 every run.
 */
const ARCHIVE_SEQ_BASE = 1_000_000;
/** Visual gap inserted between runs on the archived timeline. */
const RUN_GAP_MS = 1_000;

/**
 * Fold the live run's finalized segments onto the end of the archived
 * timeline: chronological within the run, seqs made globally unique, times
 * shifted past everything archived so ordering is stable. Pure — used both
 * when a new run starts (archiving the previous one) and when collecting
 * the full transcript for a conversation save.
 */
export function withLiveArchived(
  archived: TranscriptSegment[],
  live: TranscriptSegment[],
): TranscriptSegment[] {
  const finals = live
    .filter((s) => s.is_final && s.text.trim().length > 0)
    .sort((a, b) => a.start_ms - b.start_ms);
  if (finals.length === 0) return archived;
  const timeBase =
    archived.length > 0
      ? Math.max(...archived.map((s) => s.end_ms)) + RUN_GAP_MS
      : 0;
  let seq =
    archived.length > 0
      ? Math.max(ARCHIVE_SEQ_BASE, ...archived.map((s) => s.seq + 1))
      : ARCHIVE_SEQ_BASE;
  const shifted = finals.map((s) => ({
    ...s,
    seq: seq++,
    start_ms: s.start_ms + timeBase,
    end_ms: s.end_ms + timeBase,
  }));
  return [...archived, ...shifted];
}

interface TranscriptState {
  /** Chronological finalized + in-flight segments of the LIVE run. */
  segments: TranscriptSegment[];
  /**
   * Earlier material shown above the live run: previous runs of the open
   * conversation, or a loaded conversation's transcript.
   */
  archived: TranscriptSegment[];
  /**
   * True while a conversation is open: starting a new listen appends (the
   * current run is archived) instead of wiping the screen.
   */
  retainHistory: boolean;
  session: SessionStateEvent;
  levels: Record<StreamSide, AudioLevelEvent | null>;

  /** Non-null while browsing a past session's transcript (U3 reopen). */
  viewingPastSessionId: string | null;

  applySegment: (segment: TranscriptSegment) => void;
  setSession: (session: SessionStateEvent) => void;
  setLevel: (level: AudioLevelEvent) => void;
  setRetainHistory: (retain: boolean) => void;
  loadPastSession: (id: string, segments: TranscriptSegment[]) => void;
  /** Replace the view with a saved conversation's transcript. */
  loadConversation: (segments: TranscriptSegment[]) => void;
  clear: () => void;
}

/**
 * A final segment replaces every partial that carried the same (side, seq);
 * a newer partial replaces the previous partial for its (side, seq).
 */
export const useTranscriptStore = create<TranscriptState>((set) => ({
  segments: [],
  archived: [],
  retainHistory: false,
  session: { state: "idle" },
  levels: { inbound: null, outbound: null },
  viewingPastSessionId: null,

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

  setSession: (session) =>
    set((s) => {
      if (session.state !== "listening") return { session };
      // New live run: with a conversation open the previous run is archived
      // (append); otherwise the screen resets as before.
      return {
        session,
        archived: s.retainHistory ? withLiveArchived(s.archived, s.segments) : [],
        segments: [],
        viewingPastSessionId: null,
      };
    }),

  setRetainHistory: (retain) => set({ retainHistory: retain }),

  loadPastSession: (id, segments) =>
    set({ segments, archived: [], viewingPastSessionId: id }),

  loadConversation: (segments) =>
    set({ archived: segments, segments: [], viewingPastSessionId: null }),

  setLevel: (level) =>
    set((s) => ({ levels: { ...s.levels, [level.side]: level } })),

  clear: () => set({ segments: [], archived: [] }),
}));
