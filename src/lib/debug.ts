/**
 * Diagnostics. The build stamp proves exactly which commit is running — the
 * fastest way to tell "the fix didn't work" apart from "you're on a stale
 * build". `collectDebugReport` assembles a shareable snapshot (no secrets) for
 * pasting into a bug report or saving to a file.
 */
import { isTauri } from "@/lib/ipc";
import { useAppStore } from "@/state/app";
import { useTranscriptStore } from "@/state/transcript";

export const BUILD = {
  version: typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "0.0.0",
  sha: typeof __GIT_SHA__ === "string" ? __GIT_SHA__ : "unknown",
  time: typeof __BUILD_TIME__ === "string" ? __BUILD_TIME__ : "unknown",
};

/** `v0.1.1 · build a1b2c3` — version is what the user reports, sha is what
 *  debugging needs. Keep both (SDLC §3.3). */
export const VERSION_LABEL = `v${BUILD.version} · build ${BUILD.sha}`;

// Log once at boot so the console always shows the running version + commit.
console.log(`[conva] v${BUILD.version} · build ${BUILD.sha} · ${BUILD.time}`);

/** Live measured geometry of a layout column tagged with data-col. */
function colRect(name: string): string {
  const el = document.querySelector(
    `[data-col="${name}"]`,
  ) as HTMLElement | null;
  if (!el) return `${name}: (not mounted)`;
  const r = el.getBoundingClientRect();
  return `${name}: ${Math.round(r.width)}w × ${Math.round(r.height)}h @ x=${Math.round(r.left)}`;
}

export function collectDebugReport(): string {
  const app = useAppStore.getState();
  const t = useTranscriptStore.getState();
  const cfg = app.config;
  return [
    "conva debug report",
    `version:    v${BUILD.version}`,
    `build:      ${BUILD.sha} · ${BUILD.time}`,
    `captured:   ${new Date().toISOString()}`,
    `tauri:      ${isTauri()}`,
    `platform:   ${navigator.platform}`,
    `ua:         ${navigator.userAgent}`,
    `window:     ${window.innerWidth} × ${window.innerHeight} (dpr ${window.devicePixelRatio})`,
    "columns (live measured):",
    `  ${colRect("transcript")}`,
    `  ${colRect("spine")}`,
    `  ${colRect("ally")}`,
    `compact:    ${app.compact}`,
    `session:    ${t.session.state}`,
    `busy:       ${app.busy}`,
    `lastError:  ${app.lastError ?? "(none)"}`,
    `asr_engine: ${cfg?.asr_engine ?? "(no config)"}`,
    `whisper:    ${cfg?.whisper_model ?? "-"}`,
    `devices:    in=${cfg?.input_device ?? "default"} · loop=${cfg?.loopback_device ?? "default"}`,
  ].join("\n");
}
