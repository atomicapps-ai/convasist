import { useEffect, useState } from "react";

import { exportTranscript, sessionList, sessionLoad } from "@/lib/commands";
import type { SessionSummary } from "@/lib/ipc";
import { useTranscriptStore } from "@/state/transcript";

function formatDate(unixMs: number): string {
  if (!unixMs) return "—";
  return new Date(unixMs).toLocaleString();
}

/** Past sessions (U3): reopen a transcript, or export it as Markdown (U8). */
export function SessionsPanel({ onClose }: { onClose: () => void }) {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const loadPastSession = useTranscriptStore((s) => s.loadPastSession);
  const liveSegments = useTranscriptStore((s) => s.segments);
  const archived = useTranscriptStore((s) => s.archived);
  const segments = [...archived, ...liveSegments];
  const viewing = useTranscriptStore((s) => s.viewingPastSessionId);

  useEffect(() => {
    sessionList()
      .then(setSessions)
      .catch((e) => setNotice(String(e)));
  }, []);

  const open = async (id: string) => {
    try {
      loadPastSession(id, await sessionLoad(id));
      onClose();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const exportCurrent = async () => {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({
        defaultPath: "convasist-transcript.md",
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (!path) return;
      await exportTranscript(path, segments);
      setNotice(`Exported to ${path}`);
    } catch (e) {
      setNotice(String(e));
    }
  };

  return (
    <div className="border-b border-border bg-panel px-4 py-3">
      <div className="flex items-center gap-2">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-fg-muted">
          Sessions
        </h3>
        {viewing && (
          <span className="text-[11px] text-ai">viewing past session</span>
        )}
        <button
          type="button"
          disabled={segments.length === 0}
          onClick={() => void exportCurrent()}
          className="ml-auto rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
        >
          Export shown transcript…
        </button>
        <button
          type="button"
          onClick={onClose}
          className="rounded-md border border-border px-3 py-1 text-xs text-fg-muted hover:text-fg"
        >
          Close
        </button>
      </div>

      {notice && (
        <p className="mt-2 text-[11px] text-fg-muted" role="status">
          {notice}
        </p>
      )}

      {sessions.length === 0 ? (
        <p className="mt-3 text-xs text-fg-faint">
          No recorded sessions yet — transcripts are saved automatically while
          you listen.
        </p>
      ) : (
        <ul className="mt-2 flex max-h-48 flex-col gap-1 overflow-y-auto">
          {sessions.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                onClick={() => void open(s.id)}
                className="flex w-full items-center gap-3 rounded-md border border-border bg-bg px-3 py-1.5 text-left hover:border-fg-faint"
              >
                <span className="font-mono text-[11px] text-fg-muted">
                  {formatDate(s.started_at_unix_ms)}
                </span>
                <span className="truncate text-xs text-fg">
                  {s.preview || "(empty)"}
                </span>
                <span className="ml-auto shrink-0 font-mono text-[10px] text-fg-faint">
                  {s.segment_count} segments
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
