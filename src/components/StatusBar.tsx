import { useTranscriptStore } from "@/state/transcript";

export function StatusBar() {
  const session = useTranscriptStore((s) => s.session);
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
      <span className="ml-auto text-xs text-fg-muted">
        {session.state === "error" ? session.message : "Phase 1 · M0 scaffold"}
      </span>
    </header>
  );
}
