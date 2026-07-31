/**
 * Platform identity — the ONE ergonomic place components ask "web or desktop?".
 *
 * The product UI in `src/` is the shared BASE. Desktop and web are two builds of
 * it (siblings, not parent/child): desktop wraps it in Tauri, web ships it as
 * static files. Divergence is layered on top of the base three ways, never by
 * forking the base:
 *
 *   1. Skin      — `<html data-platform="…">` (set in main.tsx) lets the CSS in
 *                  globals.css override base tokens for one platform only.
 *   2. Behaviour — components branch on `capabilities()` (what the backend can
 *                  do) or, for pure presentation, on `PLATFORM` / `isWeb` here.
 *   3. Whole views — web-only screens live in `src/components/web/`, desktop-only
 *                  ones stay beside their feature; the shell mounts them by
 *                  platform. (This module is the guard those use.)
 *
 * Prefer `capabilities()` for "can this run here?" and reserve `PLATFORM` for
 * genuinely presentational forks (skin, marketing-style web extras).
 */
import { isTauriRuntime } from "@/lib/backend/detect";

export type Platform = "web" | "desktop";

/** Resolved once at load — the runtime never changes platform mid-session. */
export const PLATFORM: Platform = isTauriRuntime() ? "desktop" : "web";

export const isWeb = PLATFORM === "web";
export const isDesktop = PLATFORM === "desktop";

/**
 * True when the web app is embedded inside the website (an iframe under the
 * getconva.com header, or `?embed=1`). The host site then owns the top-level
 * chrome + login, so the app hides its own TopBar and doesn't self-redirect to
 * login (see App.tsx / StudioShell). Standalone /app (full-screen) keeps both.
 */
export const isEmbedded =
  typeof window !== "undefined" &&
  (new URLSearchParams(window.location.search).has("embed") ||
    window.self !== window.top);
