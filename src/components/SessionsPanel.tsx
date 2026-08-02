import { useEffect, useState } from "react";

import { useBackend } from "@/lib/backend";
import { Notice, ViewShell } from "@/components/studio/ViewShell";
import type { SessionSummary } from "@/lib/ipc";
import { useTranscriptStore } from "@/state/transcript";

function formatDate(unixMs: number): string {
  if (!unixMs) return "—";
  return new Date(unixMs).toLocaleString();
}

/** Past sessions (U3): reopen a transcript, or export it as Markdown (U8). */
export function SessionsPanel({ onClose }: { onClose: () => void }) {
  const backend = useBackend();
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const loadPastSession = useTranscriptStore((s) => s.loadPastSession);
  const liveSegments = useTranscriptStore((s) => s.segments);
  const archived = useTranscriptStore((s) => s.archived);
  const segments = [...archived, ...liveSegments];
  const viewing = useTranscriptStore((s) => s.viewingPastSessionId);

  useEffect(() => {
    backend.sessions
      .list()
      .then(setSessions)
      .catch((e) => setNotice(String(e)));
  }, [backend]);

  const open = async (id: string) => {
    try {
      loadPastSession(id, await backend.sessions.load(id));
      onClose();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const exportCurrent = async () => {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({
        defaultPath: "conva-transcript.md",
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (!path) return;
      await backend.sessions.exportTranscript(path, segments);
      setNotice(`Exported to ${path}`);
    } catch (e) {
      setNotice(String(e));
    }
  };

  return (
    <ViewShell
      icon="sessions"
      title="Sessions"
      subtitle="Every listening run is saved automatically — reopen or export a transcript."
      badge={
        viewing ? (
          <span className="rounded-full border border-ai/40 px-2 py-0.5 text-[10px] text-ai">
            viewing past session
          </span>
        ) : undefined
      }
      actions={
        <>
          <button
            type="button"
            disabled={segments.length === 0}
            onClick={() => void exportCurrent()}
            className="btn"
          >
            Export shown transcript…
          </button>
          <button type="button" onClick={onClose} className="btn">
            Done
          </button>
        </>
      }
    >
      {notice && <Notice>{notice}</Notice>}

      {sessions.length === 0 ? (
        <div className="card grid place-items-center px-6 py-16 text-center text-xs text-fg-faint">
          No recorded sessions yet — transcripts are saved automatically while
          you listen.
        </div>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {sessions.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                onClick={() => void open(s.id)}
                className="row w-full"
              >
                <span className="font-mono text-[11px] text-fg-muted">
                  {formatDate(s.started_at_unix_ms)}
                </span>
                {s.is_rehearsal && (
                  <span
                    title={
                      s.simcon_title
                        ? `Sim Con rehearsal: ${s.simcon_title}`
                        : "Sim Con rehearsal"
                    }
                    className="shrink-0 rounded-sm border border-ai/40 bg-ai/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-ai"
                  >
                    Sim Con
                  </span>
                )}
                <span className="truncate text-xs text-fg">
                  {s.is_rehearsal && s.simcon_title
                    ? s.simcon_title
                    : s.preview || "(empty)"}
                </span>
                <span className="ml-auto shrink-0 font-mono text-[10px] text-fg-faint">
                  {s.segment_count} segments
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </ViewShell>
  );
}
