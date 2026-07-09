import { useEffect, useState } from "react";

import { isTauri } from "@/lib/ipc";
import { useAssistStore, type AssistCard } from "@/state/assist";

function Card({ card }: { card: AssistCard }) {
  const label =
    card.kind === "suggest_reply"
      ? "Suggested reply"
      : card.kind === "summarize"
        ? "Summary"
        : (card.question ?? "Question");

  return (
    <div className="rounded-md border border-ai/25 bg-ai/5 px-3 py-2">
      <div className="mb-1 flex items-center gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-ai">
          {label}
        </span>
        {!card.done && (
          <span className="text-[11px] text-fg-faint" role="status">
            thinking…
          </span>
        )}
        <button
          type="button"
          onClick={() => void navigator.clipboard.writeText(card.text)}
          className="ml-auto text-[11px] text-fg-faint hover:text-fg"
        >
          Copy
        </button>
      </div>
      {card.error ? (
        <p className="text-xs text-rec">{card.error}</p>
      ) : (
        <p className="whitespace-pre-wrap text-sm leading-relaxed">
          {card.text || "…"}
        </p>
      )}
    </div>
  );
}

/**
 * AI assist dock (design §5.2, U4/O2): action buttons + streaming answer
 * cards. `Ctrl+Space` fires "suggest reply" from anywhere in the app.
 */
export function AssistDock() {
  const cards = useAssistStore((s) => s.cards);
  const busy = useAssistStore((s) => s.busy);
  const request = useAssistStore((s) => s.request);
  const [question, setQuestion] = useState("");
  const [collapsed, setCollapsed] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.code === "Space") {
        e.preventDefault();
        void request("suggest_reply");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [request]);

  if (!isTauri()) {
    return (
      <aside className="shrink-0 border-t border-border bg-panel px-4 py-2">
        <p className="text-xs text-fg-muted">
          <span className="mr-2 font-semibold text-ai" aria-hidden>
            ✦
          </span>
          AI assist is available in the desktop app.
        </p>
      </aside>
    );
  }

  const submitQuestion = () => {
    const q = question.trim();
    if (!q) return;
    setQuestion("");
    void request("question", q);
  };

  return (
    <aside
      className="max-h-[40%] shrink-0 overflow-y-auto border-t border-border bg-panel px-4 py-2"
      aria-label="AI assist"
    >
      <div className="flex items-center gap-2">
        <span className="font-semibold text-ai" aria-hidden>
          ✦
        </span>
        <button
          type="button"
          disabled={busy}
          onClick={() => void request("suggest_reply")}
          className="rounded-md border border-ai/40 px-2.5 py-1 text-xs font-semibold text-ai hover:bg-ai/10 disabled:opacity-50"
        >
          Suggest reply
        </button>
        <span className="text-[10px] text-fg-faint">Ctrl+Space</span>
        <button
          type="button"
          disabled={busy}
          onClick={() => void request("summarize")}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
        >
          Summarize
        </button>
        <input
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submitQuestion()}
          placeholder="Ask about this conversation…"
          className="min-w-0 flex-1 rounded-md border border-border bg-bg px-2 py-1 text-xs text-fg placeholder:text-fg-faint"
        />
        {cards.length > 0 && (
          <button
            type="button"
            onClick={() => setCollapsed((v) => !v)}
            className="text-[11px] text-fg-faint hover:text-fg"
          >
            {collapsed ? "Expand" : "Collapse"}
          </button>
        )}
      </div>
      {!collapsed && cards.length > 0 && (
        <div className="mt-2 flex flex-col gap-2">
          {cards.map((card) => (
            <Card key={card.id} card={card} />
          ))}
        </div>
      )}
    </aside>
  );
}
