import { useEffect } from "react";

import {
  EVENTS,
  isTauri,
  type AudioLevelEvent,
  type ModelStatusEvent,
  type SessionStateEvent,
  type TranscriptSegment,
} from "@/lib/ipc";
import { useAppStore } from "@/state/app";
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
  }, [applySegment, setSession, setLevel, setModelStatus]);
}
