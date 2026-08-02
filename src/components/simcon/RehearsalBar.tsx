import { Icon } from "@/components/ui/Icon";
import { useBackend } from "@/lib/backend";
import { useAllyStore } from "@/state/ally";
import { useAppStore } from "@/state/app";
import { useRehearsalStore } from "@/state/rehearsal";

/** Three pulsing bars — the "actively speaking" animation for the persona. */
function SpeakingBars() {
  return (
    <span className="flex h-4 items-end gap-[2px]" aria-hidden>
      {[0, 150, 300].map((d) => (
        <span
          key={d}
          className="w-[3px] animate-pulse rounded-full bg-inbound"
          style={{ height: "100%", animationDelay: `${d}ms`, animationDuration: "700ms" }}
        />
      ))}
    </span>
  );
}

/**
 * Floating controls for a live Sim Con rehearsal, shown over the cockpit while
 * one is running. Left: a phase indicator (listening / thinking / speaking) with
 * a speaking animation. Right: generate an ideal answer with Ally and "use" it
 * to drive the interview forward, hand the turn over manually, or end.
 */
export function RehearsalBar() {
  const backend = useBackend();
  const active = useRehearsalStore((s) => s.active);
  const persona = useRehearsalStore((s) => s.personaTitle) ?? "The counterparty";
  const phase = useRehearsalStore((s) => s.phase);
  const end = useRehearsalStore((s) => s.end);

  const cards = useAllyStore((s) => s.cards);
  const busy = useAllyStore((s) => s.busy);
  const request = useAllyStore((s) => s.request);

  if (!active) return null;

  // The most recent finished "suggest reply" card is the answer we can use.
  const suggestion = cards.find(
    (c) => c.kind === "suggest_reply" && c.done && !c.error && c.text.trim(),
  );

  const endRehearsal = async () => {
    try {
      // Mark the saved conversation as a Sim Con so it's identifiable in the
      // Conversations list, then route through the app store's stop so ending
      // offers to save the full transcript (both sides) — same as top-bar Stop.
      const { useConversationStore } = await import("@/state/conversation");
      if (!useConversationStore.getState().openId) {
        useConversationStore.getState().setTitle(`Sim Con — ${persona}`);
      }
      await useAppStore.getState().stop();
    } finally {
      end();
    }
  };

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-3 z-40 flex justify-center px-3">
      <div className="glass-raised pointer-events-auto flex max-w-full items-center gap-3 rounded-full border border-border-strong px-3 py-1.5 shadow-[var(--shadow-lg)]">
        {/* Phase indicator */}
        <span className="flex items-center gap-2 pl-1 text-[12px] font-semibold">
          {phase === "speaking" ? (
            <>
              <SpeakingBars />
              <span className="text-inbound">{persona} is speaking…</span>
            </>
          ) : phase === "thinking" ? (
            <>
              <Icon name="simicon" size={15} className="animate-pulse text-ai" />
              <span className="text-ai">{persona} is thinking…</span>
            </>
          ) : (
            <>
              <span className="h-2 w-2 animate-pulse rounded-full bg-rec" />
              <span className="text-[var(--voice-you-text)]">
                Your turn — speak, or use a suggested answer
              </span>
            </>
          )}
        </span>

        <span className="h-5 w-px bg-border-strong" />

        {/* Generate + use an ideal answer (Ally). */}
        <button
          type="button"
          disabled={busy}
          onClick={() => void request("suggest_reply")}
          title="Ask Ally for the ideal answer to the last question"
          className="flex h-7 items-center gap-1.5 rounded-full border border-ai/40 px-3 text-[12px] font-semibold text-ai transition hover:bg-ai/10 disabled:opacity-40"
        >
          <Icon name="lightbulb" size={14} />
          {busy ? "Thinking…" : "Suggest my answer"}
        </button>
        <button
          type="button"
          disabled={!suggestion}
          onClick={() =>
            suggestion && void backend.simcon.rehearsalSay(suggestion.text)
          }
          title={
            suggestion
              ? "Answer with Ally's suggestion — the interviewer will respond"
              : "Generate a suggested answer first"
          }
          className="flex h-7 items-center gap-1.5 rounded-full bg-ai px-3 text-[12px] font-bold text-bg transition hover:brightness-110 disabled:opacity-40"
        >
          <Icon name="chevron" size={13} className="-rotate-90" />
          Use it
        </button>

        <span className="h-5 w-px bg-border-strong" />

        <button
          type="button"
          onClick={() => void backend.simcon.rehearsalYourTurn()}
          title="End your turn now instead of waiting for a pause"
          className="flex h-7 items-center rounded-full px-2.5 text-[12px] font-semibold text-fg-muted transition hover:text-fg"
        >
          Your turn
        </button>
        <button
          type="button"
          onClick={() => void endRehearsal()}
          title="End the rehearsal"
          className="flex h-7 items-center gap-1 rounded-full border border-border px-2.5 text-[12px] font-semibold text-fg-muted transition hover:border-rec/50 hover:text-rec"
        >
          <Icon name="close" size={13} />
          End
        </button>
      </div>
    </div>
  );
}
