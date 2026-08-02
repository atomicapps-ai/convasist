import { useCallback, useEffect, useState } from "react";

import { SimConDetail } from "@/components/simcon/SimConDetail";
import { SimConSetup } from "@/components/simcon/SimConSetup";
import { Section, ViewShell } from "@/components/studio/ViewShell";
import { Icon } from "@/components/ui/Icon";
import { useBackend } from "@/lib/backend";
import type {
  SimConCategory,
  SimConSession,
  SimConStatus,
  SimConSummary,
} from "@/lib/ipc";
import { isDesktop } from "@/lib/platform";

const CATEGORY_LABEL: Record<SimConCategory, string> = {
  interview: "Interview",
  financial_review: "Financial review",
  performance_review: "Performance review",
  sales_pitch: "Sales pitch",
  other: "Other",
};

const STATUS_LABEL: Record<SimConStatus, string> = {
  draft: "Draft",
  ingesting: "Preparing…",
  ready: "Ready",
  running: "Running",
  ended: "Ended",
};

type Mode =
  | { k: "list" }
  | { k: "setup"; initial: SimConSession | null }
  | { k: "detail"; id: string };

/**
 * Sim Con — Simulated Conversation. Lists saved Sim Cons; a row opens the detail
 * (personas + rehearse), the pencil edits the setup in place, the trash deletes.
 * Desktop-first; on web it shows the honest degraded state.
 */
export function SimConView() {
  const backend = useBackend();
  const [items, setItems] = useState<SimConSummary[]>([]);
  const [mode, setMode] = useState<Mode>({ k: "list" });
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    backend.simcon
      .list()
      .then((list) => {
        setItems(list);
        setError(null);
      })
      .catch(() => setError("Sim Con runs on the desktop app for now."));
  }, [backend]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const edit = async (id: string) => {
    try {
      setMode({ k: "setup", initial: await backend.simcon.load(id) });
    } catch {
      setError("Couldn't open that Sim Con.");
    }
  };

  const remove = async (id: string) => {
    try {
      await backend.simcon.delete(id);
      refresh();
    } catch {
      /* best-effort; the list refresh reflects the real state */
    }
  };

  const backToList = () => {
    setMode({ k: "list" });
    refresh();
  };

  if (mode.k === "setup") {
    return (
      <SimConSetup
        initial={mode.initial ?? undefined}
        onDone={backToList}
        onCancel={() => setMode({ k: "list" })}
      />
    );
  }

  if (mode.k === "detail") {
    return (
      <SimConDetail
        id={mode.id}
        onEdit={() => void edit(mode.id)}
        onBack={backToList}
      />
    );
  }

  return (
    <ViewShell
      icon="simicon"
      title="Sim Con"
      subtitle="Rehearse a high-stakes call — the AI plays the other side."
      badge={
        <span className="inline-flex items-center rounded-full border border-border-strong bg-panel-raised/60 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-fg-faint">
          Preview
        </span>
      }
      actions={
        isDesktop ? (
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setMode({ k: "setup", initial: null })}
          >
            <Icon name="simicon" size={14} />
            New Sim Con
          </button>
        ) : undefined
      }
    >
      {error && (
        <Section title="Sim Con">
          <p className="text-sm text-fg-muted">{error}</p>
        </Section>
      )}

      {!error && items.length === 0 && (
        <Section title="No Sim Cons yet">
          <p className="text-sm leading-relaxed text-fg-muted">
            Create a Sim Con to rehearse an interview, review, or pitch. conva
            builds a reusable knowledge base from your library and plays the
            counterparty. Generated personas and the live session follow.
          </p>
        </Section>
      )}

      {!error && items.length > 0 && (
        <Section title="Your Sim Cons">
          <ul className="flex flex-col divide-y divide-border">
            {items.map((s) => (
              <li
                key={s.id}
                className="flex items-center gap-1 py-2 first:pt-0 last:pb-0"
              >
                <button
                  type="button"
                  onClick={() => setMode({ k: "detail", id: s.id })}
                  title="Open this Sim Con"
                  className="min-w-0 flex-1 rounded-sm px-1 py-1 text-left transition hover:bg-panel-raised/60"
                >
                  <p className="truncate text-sm font-semibold text-fg">
                    {s.title}
                  </p>
                  <p className="text-[11px] text-fg-faint">
                    {CATEGORY_LABEL[s.category]} · {STATUS_LABEL[s.status]}
                  </p>
                </button>
                <button
                  type="button"
                  onClick={() => void edit(s.id)}
                  aria-label="Edit setup"
                  title="Edit setup"
                  className="rounded-sm p-1.5 text-fg-faint transition hover:bg-panel-raised/60 hover:text-fg"
                >
                  <Icon name="edit" size={15} />
                </button>
                <button
                  type="button"
                  onClick={() => void remove(s.id)}
                  aria-label="Delete"
                  title="Delete"
                  className="rounded-sm p-1.5 text-fg-faint transition hover:bg-rec/10 hover:text-rec"
                >
                  <Icon name="trash" size={15} />
                </button>
              </li>
            ))}
          </ul>
        </Section>
      )}
    </ViewShell>
  );
}
