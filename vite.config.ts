import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// The app version, read from package.json — the single source of truth that
// `scripts/version.mjs` keeps in lockstep with Cargo.toml + tauri.conf.json and
// the release tag (SDLC §3.1/§3.3). Surfaced in the UI as `v<version>` next to
// the build sha, so the user reports the version and debugging keeps the sha.
const appVersion = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf8"),
).version as string;

// Build stamp so a running app can prove exactly which commit it is — the
// fastest way to tell "the fix didn't work" apart from "you're on a stale
// build". Resolved when the dev server / build starts.
const gitSha = (() => {
  try {
    return execSync("git rev-parse --short HEAD", {
      stdio: ["ignore", "pipe", "ignore"],
    })
      .toString()
      .trim();
  } catch {
    return "nogit";
  }
})();
const buildTime = new Date().toISOString();

// Port 1420 is what src-tauri/tauri.conf.json points `devUrl` at.
export default defineConfig({
  // Expose CONVA_* to the client so the SAME .env.dev/.env.prod files that
  // drive the desktop build (via `env/cli.mjs run`) also select the Supabase
  // backend for the web build (consumed in src/lib/backend/webAuth.ts).
  envPrefix: ["VITE_", "CONVA_"],
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
    __GIT_SHA__: JSON.stringify(gitSha),
    __BUILD_TIME__: JSON.stringify(buildTime),
  },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Never watch the Rust side: cargo holds locks on build artifacts
      // (EBUSY on Windows) and its output churn is irrelevant to the UI.
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },
  build: {
    target: "es2022",
  },
});
