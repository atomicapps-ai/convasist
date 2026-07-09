import { useState } from "react";

import { useAssistStore } from "@/state/assist";

/**
 * Pinned commitments & entities rail (design §6.3). Renders only once the
 * tracker has produced something; collapsible to a thin edge tab.
 */
export function TrackerRail() {
  const tracker = useAssistStore((s) => s.tracker);
  const [collapsed, setCollapsed] = useState(false);

  if (!tracker || (tracker.entities.length === 0 && tracker.commitments.length === 0)) {
    return null;
  }

  if (collapsed) {
    return (
      <button
        type="button"
        onClick={() => setCollapsed(false)}
        className="shrink-0 border-l border-border bg-panel px-1.5 text-[10px] font-semibold uppercase tracking-widest text-fg-faint hover:text-fg"
        aria-label="Expand tracker"
        style={{ writingMode: "vertical-rl" }}
      >
        Tracker ({tracker.commitments.length + tracker.entities.length})
      </button>
    );
  }

  return (
    <aside
      className="flex w-64 shrink-0 flex-col overflow-y-auto border-l border-border bg-panel px-3 py-2"
      aria-label="Commitments and entities"
    >
      <div className="mb-1 flex items-center">
        <h2 className="text-[11px] font-semibold uppercase tracking-widest text-fg-muted">
          Tracker
        </h2>
        <button
          type="button"
          onClick={() => setCollapsed(true)}
          className="ml-auto text-[11px] text-fg-faint hover:text-fg"
          aria-label="Collapse tracker"
        >
          »
        </button>
      </div>

      {tracker.commitments.length > 0 && (
        <>
          <h3 className="mt-1 text-[10px] font-semibold uppercase tracking-wider text-ai">
            Commitments
          </h3>
          <ul className="mt-1 flex flex-col gap-1.5">
            {tracker.commitments.map((c, i) => (
              <li key={i} className="rounded-md border border-ai/20 bg-ai/5 px-2 py-1.5">
                <p className="text-xs leading-snug">{c.what}</p>
                <p className="mt-0.5 font-mono text-[10px] text-fg-faint">
                  {c.who === "you" ? "you" : "them"}
                  {c.due ? ` · due ${c.due}` : ""}
                </p>
              </li>
            ))}
          </ul>
        </>
      )}

      {tracker.entities.length > 0 && (
        <>
          <h3 className="mt-3 text-[10px] font-semibold uppercase tracking-wider text-fg-muted">
            Mentioned
          </h3>
          <ul className="mt-1 flex flex-col gap-1">
            {tracker.entities.map((e, i) => (
              <li key={i} className="text-xs leading-snug">
                <span className="font-semibold">{e.label}</span>
                {e.detail && (
                  <span className="text-fg-muted"> — {e.detail}</span>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
    </aside>
  );
}
