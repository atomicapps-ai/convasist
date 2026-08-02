import { useEffect } from "react";

import { useBackend } from "@/lib/backend";
import { useAppStore } from "@/state/app";
import { useAllyStore } from "@/state/ally";
import { useRehearsalStore } from "@/state/rehearsal";
import { useTranscriptStore } from "@/state/transcript";

/**
 * Subscribes the stores to the backend's live event stream via the platform
 * abstraction. On desktop the Tauri adapter binds the `conva://*` events; in a
 * plain browser tab the web adapter no-ops each subscription and the UI renders
 * its empty states.
 */
export function useIpcBridge(): void {
  const backend = useBackend();
  const applySegment = useTranscriptStore((s) => s.applySegment);
  const setSession = useTranscriptStore((s) => s.setSession);
  const setLevel = useTranscriptStore((s) => s.setLevel);
  const setModelStatus = useAppStore((s) => s.setModelStatus);
  const applyAllyChunk = useAllyStore((s) => s.applyChunk);
  const applyAllySources = useAllyStore((s) => s.applySources);
  const applyRadar = useAllyStore((s) => s.applyRadar);
  const applyTracker = useAllyStore((s) => s.applyTracker);
  const applyRehearsalPhase = useRehearsalStore((s) => s.applyPhase);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    void (async () => {
      const subs = await Promise.all([
        backend.subscribe("transcriptSegment", applySegment),
        backend.subscribe("sessionState", setSession),
        backend.subscribe("audioLevel", setLevel),
        backend.subscribe("modelStatus", setModelStatus),
        backend.subscribe("allyChunk", applyAllyChunk),
        backend.subscribe("allySources", applyAllySources),
        backend.subscribe("radar", applyRadar),
        backend.subscribe("tracker", applyTracker),
        backend.subscribe("rehearsalState", applyRehearsalPhase),
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
    backend,
    applySegment,
    setSession,
    setLevel,
    setModelStatus,
    applyAllyChunk,
    applyAllySources,
    applyRadar,
    applyTracker,
    applyRehearsalPhase,
  ]);
}
