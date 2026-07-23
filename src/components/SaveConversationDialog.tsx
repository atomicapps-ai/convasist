import { useEffect, useState } from "react";

import { useConversationStore } from "@/state/conversation";
import { useTranscriptStore } from "@/state/transcript";

/** First words of the transcript as a title suggestion. */
function suggestTitle(): string {
  const t = useTranscriptStore.getState();
  const first = [...t.archived, ...t.segments].find(
    (s) => s.is_final && s.text.trim().length > 0,
  );
  if (!first) return `Conversation ${new Date().toLocaleDateString()}`;
  const text = first.text.trim();
  return text.length > 48 ? `${text.slice(0, 48)}…` : text;
}

/**
 * Stop → "save this conversation?". Saving an already-open conversation
 * appends (the stored record is replaced by the fuller transcript); a
 * fresh one is created otherwise.
 */
export function SaveConversationDialog() {
  const open = useConversationStore((s) => s.savePromptOpen);
  const setOpen = useConversationStore((s) => s.setSavePromptOpen);
  const openId = useConversationStore((s) => s.openId);
  const existingTitle = useConversationStore((s) => s.title);
  const save = useConversationStore((s) => s.save);
  const setNotice = useConversationStore((s) => s.setNotice);

  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setTitle(existingTitle ?? suggestTitle());
      setError(null);
    }
  }, [open, existingTitle]);

  if (!open) return null;

  const doSave = async () => {
    setBusy(true);
    setError(null);
    try {
      await save(title);
      setOpen(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-bg/70 backdrop-blur-sm">
      <div className="w-[26rem] max-w-[90vw] rounded-lg border border-border bg-panel p-4 shadow-xl">
        <h2 className="text-sm font-semibold text-fg">
          {openId ? "Save conversation (append)?" : "Save this conversation?"}
        </h2>
        <p className="mt-1 text-xs text-fg-muted">
          {openId
            ? "This conversation is open — saving adds everything recorded since the last save to the same record."
            : "Saved conversations can be reopened, continued, and linked to library documents."}
        </p>
        <label className="mt-3 block text-[11px] text-fg-faint">
          Title
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            // eslint-disable-next-line jsx-a11y/no-autofocus
            autoFocus
            className="mt-1 w-full rounded-md border border-border bg-bg px-2 py-1.5 text-xs text-fg"
          />
        </label>
        {error && (
          <p className="mt-2 text-[11px] text-rec" role="alert">
            {error}
          </p>
        )}
        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              setOpen(false);
              setNotice(null);
            }}
            className="rounded-md border border-border px-3 py-1 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
          >
            Don&apos;t save
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void doSave()}
            className="rounded-md bg-ok/90 px-3 py-1 text-xs font-semibold text-bg hover:bg-ok disabled:opacity-50"
          >
            {busy ? "Saving…" : openId ? "Save (append)" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
