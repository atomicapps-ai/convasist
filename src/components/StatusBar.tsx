import { isTauri } from "@/lib/ipc";
import { useAppStore } from "@/state/app";
import { useTranscriptStore } from "@/state/transcript";

export function StatusBar({
  onToggleSettings,
  onToggleLibrary,
  onToggleSessions,
}: {
  onToggleSettings: () => void;
  onToggleLibrary: () => void;
  onToggleSessions: () => void;
}) {
  const session = useTranscriptStore((s) => s.session);
  const busy = useAppStore((s) => s.busy);
  const lastError = useAppStore((s) => s.lastError);
  const modelStatus = useAppStore((s) => s.modelStatus);
  const start = useAppStore((s) => s.start);
  const stop = useAppStore((s) => s.stop);
  const listening = session.state === "listening";

  const statusText = (() => {
    if (modelStatus?.state === "downloading") {
      return `Downloading speech model ${modelStatus.model}… ${modelStatus.percent}%`;
    }
    if (modelStatus?.state === "error") {
      return `Model download failed: ${modelStatus.message}`;
    }
    if (lastError === "consent_required") {
      return "Acknowledge the consent notice first.";
    }
    if (lastError?.includes("model_downloading")) {
      return "Fetching the speech model — Start again when it's ready.";
    }
    if (session.state === "error") return session.message;
    if (modelStatus?.state === "ready" && !listening) {
      return "Speech model ready — press Start listening.";
    }
    return lastError ?? "";
  })();
  const isError =
    modelStatus?.state === "error" ||
    session.state === "error" ||
    lastError === "consent_required" ||
    (lastError !== null && !lastError.includes("model_downloading"));

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

      <span
        className={`ml-auto max-w-[50%] truncate text-xs ${isError ? "text-rec" : "text-fg-muted"}`}
        role="status"
        aria-live="polite"
      >
        {statusText}
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
            onClick={onToggleSessions}
            aria-label="Past sessions and export"
            className="rounded-md border border-border px-2 py-1 text-xs text-fg-muted hover:text-fg"
          >
            Sessions
          </button>
          <button
            type="button"
            onClick={onToggleLibrary}
            aria-label="Reference document library"
            className="rounded-md border border-border px-2 py-1 text-xs text-fg-muted hover:text-fg"
          >
            Library
          </button>
          <button
            type="button"
            onClick={onToggleSettings}
            aria-label="Devices and AI settings"
            className="rounded-md border border-border px-2 py-1 text-xs text-fg-muted hover:text-fg"
          >
            Settings
          </button>
        </>
      )}
    </header>
  );
}
