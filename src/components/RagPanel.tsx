import { useCallback, useEffect, useState } from "react";

import { ragDelete, ragIngest, ragList, ragSetEnabled } from "@/lib/commands";
import type { RagDocument } from "@/lib/ipc";

const SUPPORTED = ["pdf", "docx", "md", "markdown", "txt", "html", "htm"];

/**
 * Reference-document library (design §4.4 U5/R1): drag-drop or pick files,
 * per-document enable toggle, delete. Enabled documents ground every AI
 * assist via BM25 retrieval with source attribution.
 */
export function RagPanel({ onClose }: { onClose: () => void }) {
  const [documents, setDocuments] = useState<RagDocument[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setDocuments(await ragList());
    } catch (e) {
      setNotice(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const ingest = useCallback(
    async (paths: string[]) => {
      const usable = paths.filter((p) =>
        SUPPORTED.includes(p.split(".").pop()?.toLowerCase() ?? ""),
      );
      if (usable.length === 0) {
        setNotice("No supported files (pdf, docx, md, txt, html).");
        return;
      }
      setBusy(true);
      setNotice(`Ingesting ${usable.length} file(s)…`);
      try {
        const reports = await ragIngest(usable);
        const warnings = reports.flatMap((r) => r.warnings);
        setNotice(
          warnings.length > 0
            ? `Done with warnings: ${warnings.join("; ")}`
            : `Added ${reports.length} document(s).`,
        );
        await refresh();
      } catch (e) {
        setNotice(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  // Native drag-drop delivers OS file paths through the webview (U5).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const stop = await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "over") setDragOver(true);
        if (event.payload.type === "leave") setDragOver(false);
        if (event.payload.type === "drop") {
          setDragOver(false);
          void ingest(event.payload.paths);
        }
      });
      if (cancelled) stop();
      else unlisten = stop;
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [ingest]);

  const pickFiles = async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({
      multiple: true,
      filters: [{ name: "Documents", extensions: [...SUPPORTED] }],
    });
    if (picked) void ingest(Array.isArray(picked) ? picked : [picked]);
  };

  return (
    <div
      className={`border-b border-border bg-panel px-4 py-3 ${dragOver ? "outline outline-2 -outline-offset-2 outline-ai/60" : ""}`}
    >
      <div className="flex items-center gap-2">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-fg-muted">
          Reference library
        </h3>
        <span className="text-[11px] text-fg-faint">
          drop files anywhere, or
        </span>
        <button
          type="button"
          disabled={busy}
          onClick={() => void pickFiles()}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
        >
          Add documents…
        </button>
        <button
          type="button"
          onClick={onClose}
          className="ml-auto rounded-md border border-border px-3 py-1 text-xs text-fg-muted hover:text-fg"
        >
          Close
        </button>
      </div>

      {notice && (
        <p className="mt-2 text-[11px] text-fg-muted" role="status">
          {notice}
        </p>
      )}

      {documents.length === 0 ? (
        <p className="mt-3 text-xs text-fg-faint">
          No documents yet. Add pricing sheets, product docs, or notes — the
          AI grounds its answers in them and cites the source.
        </p>
      ) : (
        <ul className="mt-2 flex flex-col gap-1">
          {documents.map((doc) => (
            <li
              key={doc.id}
              className="flex items-center gap-3 rounded-md border border-border bg-bg px-3 py-1.5"
            >
              <label className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={doc.enabled}
                  onChange={(e) =>
                    void ragSetEnabled(doc.id, e.target.checked).then(refresh)
                  }
                  aria-label={`Include ${doc.file_name} in retrieval`}
                />
                <span className={doc.enabled ? "text-fg" : "text-fg-faint line-through"}>
                  {doc.file_name}
                </span>
              </label>
              <span className="font-mono text-[10px] text-fg-faint">
                {doc.chunk_count} chunks
              </span>
              <button
                type="button"
                onClick={() => void ragDelete(doc.id).then(refresh)}
                className="ml-auto text-[11px] text-fg-faint hover:text-rec"
                aria-label={`Delete ${doc.file_name}`}
              >
                Delete
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
