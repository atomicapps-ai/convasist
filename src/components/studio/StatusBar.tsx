import { useBackend, type ConvaBackend } from "@/lib/backend";
import { BUILD, collectDebugReport } from "@/lib/debug";
import { isTauri } from "@/lib/ipc";
import { useAppStore } from "@/state/app";
import { useConversationStore } from "@/state/conversation";

/**
 * Thin ambient status strip (~26px) along the window foot. Read-only signals
 * that don't belong in the top action bar: the privacy posture, the speech
 * engine, and autosave state. Latency HUD and, later, credits/account surface
 * here too as those land (kept out until they carry real values).
 */
function Sep() {
  return <span className="h-3 w-px bg-border" aria-hidden />;
}

/** Copy a diagnostics snapshot to the clipboard and, in the app, write it to a
 *  log file too — so "it didn't work" comes with real, shareable data. */
async function dumpDebug(backend: ConvaBackend) {
  const report = collectDebugReport();
  try {
    await navigator.clipboard.writeText(report);
  } catch {
    /* clipboard may be blocked; the file write below is the fallback */
  }
  if (isTauri()) {
    try {
      const path = await backend.diagnostics.saveDebugLog(report);
      window.alert(`Debug report copied to clipboard and saved to:\n${path}`);
      return;
    } catch (e) {
      window.alert(`Debug report copied. (Saving a file failed: ${String(e)})`);
      return;
    }
  }
  window.alert("Debug report copied to clipboard.");
}

export function StatusBar() {
  const backend = useBackend();
  const config = useAppStore((s) => s.config);
  const title = useConversationStore((s) => s.title);
  const cloud = config?.asr_engine === "deepgram_cloud";

  return (
    <footer
      className="glass flex h-[26px] shrink-0 items-center gap-3 border-b-0 px-4 text-[11px] text-fg-faint"
      aria-label="Status"
    >
      <span className="flex items-center gap-1.5 text-ok" title="Audio and transcripts stay on this device">
        <svg
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.7"
          strokeLinecap="round"
        >
          <rect x="5" y="11" width="14" height="9" rx="2" />
          <path d="M8 11V8a4 4 0 0 1 8 0v3" />
        </svg>
        On-device · Private
      </span>

      {config && (
        <>
          <Sep />
          <span className="flex items-center gap-1.5">
            <span
              className={`h-1.5 w-1.5 rounded-full ${cloud ? "bg-ai" : "bg-ok"}`}
            />
            {cloud ? "Cloud · Deepgram" : "Local · whisper"}
          </span>
        </>
      )}

      {title && (
        <>
          <Sep />
          <span className="min-w-0 max-w-[20rem] truncate">Saved · {title}</span>
        </>
      )}

      {/* Right side: build stamp + one-click diagnostics. */}
      <span className="ml-auto" />
      {!isTauri() && <span className="text-fg-faint">preview</span>}
      <button
        type="button"
        onClick={() => void dumpDebug(backend)}
        title="Copy a diagnostics snapshot (and save a log file) for sharing"
        className="rounded px-1.5 py-0.5 font-mono text-[10px] text-fg-faint transition hover:bg-white/[0.06] hover:text-fg"
      >
        v{BUILD.version} · build {BUILD.sha} · debug ⧉
      </button>
    </footer>
  );
}
