import { isTauri } from "@/lib/ipc";
import { useAppStore } from "@/state/app";
import { useTranscriptStore } from "@/state/transcript";

export function StatusBar({
  onToggleSettings,
}: {
  onToggleSettings: () => void;
}) {
  const session = useTranscriptStore((s) => s.session);
  const busy = useAppStore((s) => s.busy);
  const lastError = useAppStore((s) => s.lastError);
  const start = useAppStore((s) => s.start);
  const stop = useAppStore((s) => s.stop);
  const listening = session.state === "listening";

  return (
    <header className="flex h-10 shrink-0 items-center gap-3 border-b border-border bg-panel px-4">
      <span className="flex items-center gap-1.5 font-mono text-xs">
        <span
          className={
            listening
              ? "h-2 w-2 animate-pulse rounded-full bg-rec"
              : "h-2 w-2 rounded-full bg-fg-faint"
          }
          aria-hidden
        />
        <span className={listening ? "text-rec" : "text-fg-faint"}>
          {listening ? "REC" : "IDLE"}
        </span>
      </span>
      <h1 className="text-sm font-semibold tracking-tight">convasist</h1>

      <span className="ml-auto max-w-[40%] truncate text-xs text-rec">
        {lastError === "consent_required"
          ? "Acknowledge the consent notice first."
          : session.state === "error"
            ? session.message
            : (lastError ?? "")}
      </span>

      {isTauri() && (
        <>
          <button
            type="button"
            disabled={busy}
            onClick={() => void (listening ? stop() : start())}
            className={[
              "rounded-md px-3 py-1 text-xs font-semibold disabled:opacity-50",
              listening
                ? "border border-rec/50 text-rec hover:bg-rec/10"
                : "bg-ok/90 text-bg hover:bg-ok",
            ].join(" ")}
          >
            {listening ? "Stop" : "Start listening"}
          </button>
          <button
            type="button"
            onClick={onToggleSettings}
            aria-label="Audio device settings"
            className="rounded-md border border-border px-2 py-1 text-xs text-fg-muted hover:text-fg"
          >
            Devices
          </button>
        </>
      )}
    </header>
  );
}
