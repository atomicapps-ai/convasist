import { useCallback, useEffect, useState } from "react";

import {
  conversationDelete,
  conversationList,
  conversationLoad,
} from "@/lib/commands";
import type { ConversationSummary } from "@/lib/ipc";
import { useConversationStore } from "@/state/conversation";

function formatDate(unixMs: number): string {
  if (!unixMs) return "—";
  return new Date(unixMs).toLocaleString();
}

/**
 * Open/save menu for named conversations (owner request): open one to view
 * and continue it (new runs append), save the current one, start fresh.
 */
export function ConversationsPanel({ onClose }: { onClose: () => void }) {
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const openId = useConversationStore((s) => s.openId);
  const title = useConversationStore((s) => s.title);
  const notice = useConversationStore((s) => s.notice);
  const setNotice = useConversationStore((s) => s.setNotice);
  const openConversation = useConversationStore((s) => s.openConversation);
  const newConversation = useConversationStore((s) => s.newConversation);
  const setSavePromptOpen = useConversationStore((s) => s.setSavePromptOpen);

  const refresh = useCallback(async () => {
    try {
      setConversations(await conversationList());
    } catch (e) {
      setNotice(String(e));
    }
  }, [setNotice]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const open = async (id: string) => {
    try {
      openConversation(await conversationLoad(id));
      onClose();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const remove = async (id: string) => {
    try {
      await conversationDelete(id);
      if (openId === id) newConversation();
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  return (
    <div className="border-b border-border bg-panel px-4 py-3">
      <div className="flex items-center gap-2">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-fg-muted">
          Conversations
        </h3>
        {openId && (
          <span className="truncate text-[11px] text-ai">open: {title}</span>
        )}
        <button
          type="button"
          onClick={() => setSavePromptOpen(true)}
          className="ml-auto rounded-md border border-ai/60 px-2.5 py-1 text-xs text-fg hover:bg-ai/10"
        >
          Save current…
        </button>
        <button
          type="button"
          onClick={() => {
            newConversation();
            setNotice("Started a new conversation.");
          }}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg"
        >
          New
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

      {conversations.length === 0 ? (
        <p className="mt-3 text-xs text-fg-faint">
          No saved conversations yet — press Stop after listening and choose
          Save, or use “Save current…”.
        </p>
      ) : (
        <ul className="mt-2 flex max-h-48 flex-col gap-1 overflow-y-auto">
          {conversations.map((c) => (
            <li key={c.id} className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => void open(c.id)}
                className={[
                  "flex min-w-0 flex-1 items-center gap-3 rounded-md border bg-bg px-3 py-1.5 text-left hover:border-fg-faint",
                  c.id === openId ? "border-ai/60" : "border-border",
                ].join(" ")}
              >
                <span className="truncate text-xs text-fg">{c.title}</span>
                <span className="font-mono text-[11px] text-fg-muted">
                  {formatDate(c.updated_at_unix_ms)}
                </span>
                <span className="ml-auto shrink-0 font-mono text-[10px] text-fg-faint">
                  {c.segment_count} segments
                  {c.linked_docs.length > 0 &&
                    ` · ${c.linked_docs.length} linked doc(s)`}
                </span>
              </button>
              <button
                type="button"
                onClick={() => void remove(c.id)}
                aria-label={`Delete conversation ${c.title}`}
                className="shrink-0 text-[11px] text-fg-faint hover:text-rec"
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
