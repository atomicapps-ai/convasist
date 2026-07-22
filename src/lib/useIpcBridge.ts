import { useEffect } from "react";

import {
  EVENTS,
  isTauri,
  type AssistChunkEvent,
  type AssistSourcesEvent,
  type AudioLevelEvent,
  type ModelStatusEvent,
  type RadarEvent,
  type SessionStateEvent,
  type TrackerEvent,
  type TranscriptSegment,
} from "@/lib/ipc";
import { useAppStore } from "@/state/app";
import { useAssistStore } from "@/state/assist";
import { useTranscriptStore } from "@/state/transcript";

/**
 * Subscribes the transcript store to the Rust core's event stream.
 * In a plain browser tab (UI development without the shell) this is a no-op
 * and the UI renders its empty states.
 */
export function useIpcBridge(): void {
  const applySegment = useTranscriptStore((s) => s.applySegment);
  const setSession = useTranscriptStore((s) => s.setSession);
  const setLevel = useTranscriptStore((s) => s.setLevel);
  const setModelStatus = useAppStore((s) => s.setModelStatus);
  const applyAssistChunk = useAssistStore((s) => s.applyChunk);
  const applyAssistSources = useAssistStore((s) => s.applySources);
  const applyRadar = useAssistStore((s) => s.applyRadar);
  const applyTracker = useAssistStore((s) => s.applyTracker);

  useEffect(() => {
    if (!isTauri()) return;

    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const subs = await Promise.all([
        listen<TranscriptSegment>(EVENTS.transcriptSegment, (e) =>
          applySegment(e.payload),
        ),
        listen<SessionStateEvent>(EVENTS.sessionState, (e) =>
          setSession(e.payload),
        ),
        listen<AudioLevelEvent>(EVENTS.audioLevel, (e) => setLevel(e.payload)),
        listen<ModelStatusEvent>(EVENTS.modelStatus, (e) =>
          setModelStatus(e.payload),
        ),
        listen<AssistChunkEvent>(EVENTS.assistChunk, (e) =>
          applyAssistChunk(e.payload),
        ),
        listen<AssistSourcesEvent>(EVENTS.assistSources, (e) =>
          applyAssistSources(e.payload),
        ),
        listen<RadarEvent>(EVENTS.radar, (e) => applyRadar(e.payload)),
        listen<TrackerEvent>(EVENTS.tracker, (e) => applyTracker(e.payload)),
      ]);
      if (cancelled) {
        subs.forEach((un) => un());
      } else {
        unlisteners.push(...subs);
      }
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((un) => un());
    };
  }, [
    applySegment,
    setSession,
    setLevel,
    setModelStatus,
    applyAssistChunk,
    applyAssistSources,
    applyRadar,
    applyTracker,
  ]);
}
