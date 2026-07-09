/**
 * AI assist dock (design §5.2) — suggestion cards stream in here.
 * M0 renders the collapsed placeholder; M3 wires it to assist-chunk events.
 */
export function AssistDock() {
  return (
    <aside
      className="shrink-0 border-t border-border bg-panel px-4 py-2"
      aria-label="AI assist"
    >
      <p className="text-xs text-fg-muted">
        <span className="mr-2 font-semibold text-ai" aria-hidden>
          ✦
        </span>
        AI assist arrives in M3 — grounded answers with source attribution
        will stream here.
      </p>
    </aside>
  );
}
