import { useCallback, useEffect, useState } from "react";

import {
  ragDelete,
  ragDownload,
  ragIngest,
  ragIngestText,
  ragList,
  ragSetEnabled,
} from "@/lib/commands";
import type { RagDocument } from "@/lib/ipc";

const SUPPORTED = ["pdf", "docx", "md", "markdown", "txt", "html", "htm"];

/** Name a pasted note from its first non-empty line (else a fallback). */
function deriveNoteName(text: string): string {
  const firstLine = text
    .split("\n")
    .map((l) => l.trim())
    .find((l) => l.length > 0);
  if (!firstLine) return "Pasted note";
  return firstLine.length > 60 ? `${firstLine.slice(0, 60)}…` : firstLine;
}

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
  const [pasteOpen, setPasteOpen] = useState(false);
  const [pasteText, setPasteText] = useState("");

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

  // Best-effort one-click read of the OS clipboard. The webview may block it
  // without a user gesture or on permission; the textarea (Ctrl+V) always
  // works, so failure just nudges the user there.
  const readClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (text.trim()) {
        setPasteText(text);
        setNotice(null);
      } else {
        setNotice("Clipboard is empty.");
      }
    } catch {
      setNotice("Couldn't read the clipboard automatically — paste into the box with Ctrl+V.");
    }
  };

  const downloadDoc = async (doc: RagDocument) => {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const dest = await save({ defaultPath: doc.file_name });
    if (!dest) return;
    try {
      await ragDownload(doc.id, dest);
      setNotice(`Downloaded ${doc.file_name}.`);
    } catch (e) {
      setNotice(String(e));
    }
  };

  const savePaste = async () => {
    const text = pasteText.trim();
    if (!text) {
      setNotice("Nothing to add — paste or type some text first.");
      return;
    }
    setBusy(true);
    setNotice("Adding pasted text…");
    try {
      const report = await ragIngestText(deriveNoteName(text), text);
      setNotice(
        report.warnings.length > 0
          ? `Added with warnings: ${report.warnings.join("; ")}`
          : `Added "${report.document.file_name}".`,
      );
      setPasteText("");
      setPasteOpen(false);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
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
          disabled={busy}
          onClick={() => {
            setPasteOpen((v) => !v);
            setNotice(null);
          }}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
        >
          Paste text…
        </button>
        <button
          type="button"
          onClick={onClose}
          className="ml-auto rounded-md border border-border px-3 py-1 text-xs text-fg-muted hover:text-fg"
        >
          Close
        </button>
      </div>

      {pasteOpen && (
        <div className="mt-3 rounded-md border border-border bg-bg p-2">
          <div className="mb-2 flex items-center gap-2">
            <span className="text-[11px] text-fg-faint">
              Paste (Ctrl+V) or type — saved as a .txt in the library
            </span>
            <button
              type="button"
              onClick={() => void readClipboard()}
              className="ml-auto rounded-md border border-border px-2 py-0.5 text-[11px] text-fg-muted hover:text-fg"
            >
              Paste from clipboard
            </button>
          </div>
          <textarea
            value={pasteText}
            onChange={(e) => setPasteText(e.target.value)}
            // eslint-disable-next-line jsx-a11y/no-autofocus
            autoFocus
            rows={5}
            placeholder="Paste notes, a snippet, an email… the AI will ground answers in it."
            className="w-full resize-y rounded-md border border-border bg-panel px-2 py-1.5 text-xs text-fg placeholder:text-fg-faint"
          />
          <div className="mt-2 flex items-center gap-2">
            <button
              type="button"
              disabled={busy || pasteText.trim().length === 0}
              onClick={() => void savePaste()}
              className="rounded-md border border-ai/60 px-2.5 py-1 text-xs text-fg hover:bg-ai/10 disabled:opacity-50"
            >
              Add to library
            </button>
            <button
              type="button"
              onClick={() => {
                setPasteOpen(false);
                setPasteText("");
              }}
              className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

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
                onClick={() => void downloadDoc(doc)}
                className="ml-auto text-[11px] text-fg-faint hover:text-fg"
                aria-label={`Download ${doc.file_name}`}
              >
                Download
              </button>
              <button
                type="button"
                onClick={() => void ragDelete(doc.id).then(refresh)}
                className="text-[11px] text-fg-faint hover:text-rec"
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
