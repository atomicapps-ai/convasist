import { useCallback, useEffect, useState } from "react";

import { AllySettings } from "@/components/AllySettings";
import { Notice, Section, ViewShell } from "@/components/studio/ViewShell";
import { useBackend } from "@/lib/backend";
import { BUILD } from "@/lib/debug";
import type {
  AuthStatus,
  SecretsStatus,
  StreamSide,
  WhisperModelInfo,
} from "@/lib/ipc";
import { isTauri } from "@/lib/ipc";
import { isWeb } from "@/lib/platform";
import { useAppStore } from "@/state/app";
import { useNavStore } from "@/state/nav";

/**
 * About — the exact build the user is running (version + commit + build time),
 * plus a jump to What's New. Belongs in Settings so a bug report can quote it;
 * mirrors the status-bar stamp (SDLC §3.3). Rendered in both the configured and
 * web-fallback Settings branches.
 */
function AboutSection() {
  return (
    <Section
      title="About"
      description="The exact build you're running — include this in any bug report."
    >
      <dl className="grid grid-cols-[6rem_1fr] gap-x-4 gap-y-1 text-[12px]">
        <dt className="text-fg-faint">Version</dt>
        <dd className="font-mono text-fg">v{BUILD.version}</dd>
        <dt className="text-fg-faint">Build</dt>
        <dd className="font-mono text-fg">{BUILD.sha}</dd>
        <dt className="text-fg-faint">Built</dt>
        <dd className="font-mono text-fg-muted">{BUILD.time}</dd>
      </dl>
      <button
        type="button"
        onClick={() => useNavStore.getState().setView("releases")}
        className="btn mt-3"
      >
        What's new →
      </button>
    </Section>
  );
}

function DeviceSelect({
  side,
  label,
  value,
  onChange,
}: {
  side: StreamSide;
  label: string;
  value: string | null;
  onChange: (device: string | null) => void;
}) {
  const devices = useAppStore((s) => s.devices).filter((d) => d.side === side);
  return (
    <label className="field flex-1">
      {label}
      <select
        className="select"
        value={value ?? ""}
        onChange={(e) => onChange(e.target.value === "" ? null : e.target.value)}
      >
        <option value="">System default</option>
        {devices.map((d) => (
          <option key={d.id} value={d.id}>
            {d.name}
            {d.is_default ? " (default)" : ""}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Speech-to-text model picker — the biggest lever on transcription latency.
 *  Faster/quantized models cut delay; the change applies on the next session
 *  start (and downloads the model first if it isn't present). */
function AsrModelSelect() {
  const backend = useBackend();
  const config = useAppStore((s) => s.config);
  const updateConfig = useAppStore((s) => s.updateConfig);
  const [models, setModels] = useState<WhisperModelInfo[]>([]);

  useEffect(() => {
    if (!isTauri()) return;
    void backend.audio
      .listWhisperModels()
      .then(setModels)
      .catch(() => setModels([]));
  }, [backend]);

  if (!config) return null;
  const current = models.find((m) => m.id === config.whisper_model);

  return (
    <div className="flex items-end gap-3">
      <label className="field">
        Speech-to-text model (speed vs. accuracy)
        <select
          className="select"
          value={config.whisper_model}
          onChange={(e) => void updateConfig({ whisper_model: e.target.value })}
        >
          {/* Show the saved model even if it isn't in the curated list. */}
          {!current && (
            <option value={config.whisper_model}>{config.whisper_model}</option>
          )}
          {models.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label} (~{m.approx_mb} MB)
            </option>
          ))}
        </select>
      </label>
      <p className="min-w-0 flex-1 pb-1 text-[11px] text-fg-faint">
        {current?.note ??
          "Applies on the next session start; downloads the model if it isn't already saved."}
      </p>
    </div>
  );
}

/** Transcription engine choice: local whisper (private, free) vs Deepgram
 *  cloud streaming (~200 ms interims — true conversation speed; audio leaves
 *  the machine; needs an API key). Applies on the next session start. */
function EngineSelect() {
  const backend = useBackend();
  const config = useAppStore((s) => s.config);
  const updateConfig = useAppStore((s) => s.updateConfig);
  const [hasKey, setHasKey] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    void backend.audio.deepgramKeyStatus().then(setHasKey).catch(() => {});
  }, [backend]);

  if (!config) return null;
  const cloud = config.asr_engine === "deepgram_cloud";

  const saveKey = async () => {
    const next = keyDraft.trim();
    try {
      await backend.audio.setDeepgramKey(next);
      setHasKey(next.length > 0);
      setKeyDraft("");
      if (next.length === 0 && cloud) {
        // No key ⇒ the box can't stay checked — drop back to local whisper.
        await updateConfig({ asr_engine: "whisper_local" });
        setNotice("Key cleared — back on local whisper.");
      } else {
        setNotice(next ? "Key saved. Check the box to switch engines." : "Key cleared.");
      }
    } catch (e) {
      setNotice(String(e));
    }
  };

  return (
    <div className="rounded-lg border border-border bg-bg/40 px-3 py-2.5">
      <label
        className={`flex items-center gap-2 text-xs ${hasKey || cloud ? "text-fg" : "text-fg-faint"}`}
        title={
          hasKey || cloud
            ? "Streams audio to Deepgram for ~200 ms live captions"
            : "Save a Deepgram API key below to enable"
        }
      >
        <input
          type="checkbox"
          checked={cloud}
          disabled={!hasKey && !cloud}
          onChange={(e) =>
            void updateConfig({
              asr_engine: e.target.checked ? "deepgram_cloud" : "whisper_local",
            })
          }
        />
        Use Deepgram cloud transcription — fastest (conversation speed)
      </label>
      <p className="mt-1 pl-6 text-[11px] text-fg-faint">
        {cloud
          ? "On: ~200 ms live captions; audio streams to Deepgram. Applies on the next session start; falls back to local whisper if unreachable."
          : "Off: everything stays on this machine (local whisper). Add a key from deepgram.com (free tier) to unlock the checkbox."}
      </p>
      <div className="mt-2 flex items-center gap-2">
        <input
          type="password"
          value={keyDraft}
          onChange={(e) => setKeyDraft(e.target.value)}
          placeholder={
            hasKey ? "Key saved — paste to replace, empty to clear" : "Deepgram API key"
          }
          className="input min-w-0 flex-1"
        />
        <button
          type="button"
          onClick={() => void saveKey()}
          className="btn shrink-0"
        >
          Save key
        </button>
        <span className="text-[11px] text-fg-faint" role="status">
          {notice ?? (hasKey ? "Key on file ✓" : "No key yet")}
        </span>
      </div>
    </div>
  );
}

/** Neural noise filter (Silero VAD): only transcribe real speech, so fans /
 *  keyboards / a TV don't trigger wasted work and hallucinated lines. */
function NoiseFilterControls() {
  const config = useAppStore((s) => s.config);
  const updateConfig = useAppStore((s) => s.updateConfig);
  if (!config) return null;

  return (
    <div>
      <label className="flex items-center gap-2 text-xs text-fg-muted">
        <input
          type="checkbox"
          checked={config.vad_neural}
          onChange={(e) => void updateConfig({ vad_neural: e.target.checked })}
        />
        Filter background noise (neural VAD) — only transcribe speech
      </label>
      {config.vad_neural && (
        <div className="mt-1.5 flex items-center gap-2">
          <span className="text-[11px] text-fg-faint">Noise-filter strength</span>
          <input
            type="range"
            min={0}
            max={1}
            step={0.05}
            value={config.vad_sensitivity}
            onChange={(e) =>
              void updateConfig({ vad_sensitivity: Number(e.target.value) })
            }
            aria-label="Noise filtering strength"
            className="h-1 w-40 accent-ai"
          />
          <span className="font-mono text-[10px] text-fg-faint">
            {Math.round(config.vad_sensitivity * 100)}%
          </span>
          <span className="text-[11px] text-fg-faint">
            higher = stricter (rejects more noise, may clip soft speech)
          </span>
        </div>
      )}
    </div>
  );
}

/** Config file sync: settings live in `conva.config.json`, committed to
 *  the repo — a fresh machine seeds from it automatically; these buttons
 *  push/pull your current settings to/from that file. Keys are NOT in this
 *  file (they use the encrypted secrets flow below). */
function ConfigFileControls() {
  const backend = useBackend();
  const updateConfig = useAppStore((s) => s.updateConfig);
  const [notice, setNotice] = useState<string | null>(null);

  const doExport = async () => {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({
        defaultPath: "conva.config.json",
        filters: [{ name: "Config", extensions: ["json"] }],
      });
      if (!dest) return;
      await backend.config.export(dest);
      setNotice("Saved. Commit it to the repo so other machines pick it up.");
    } catch (e) {
      setNotice(String(e));
    }
  };

  const doImport = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const src = await open({
        multiple: false,
        filters: [{ name: "Config", extensions: ["json"] }],
      });
      if (!src || Array.isArray(src)) return;
      const config = await backend.config.import(src);
      // Push the imported values through the store so the UI reflects them.
      await updateConfig(config);
      setNotice("Config applied.");
    } catch (e) {
      setNotice(String(e));
    }
  };

  return (
    <div className="flex items-center gap-2">
      <button
        type="button"
        onClick={() => void doExport()}
        className="btn"
      >
        Export settings…
      </button>
      <button
        type="button"
        onClick={() => void doImport()}
        className="btn"
      >
        Import…
      </button>
      {notice && (
        <span className="text-[11px] text-fg-faint" role="status">
          {notice}
        </span>
      )}
    </div>
  );
}

/** Portable encrypted secrets: export API keys to a git-committable file and
 *  load them on another machine. The passphrase comes from an env var, so the
 *  file is safe to commit and keys never re-typed per launch. */
function SecretsSettings() {
  const backend = useBackend();
  const [status, setStatus] = useState<SecretsStatus | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setStatus(await backend.secrets.status());
    } catch (e) {
      setNotice(String(e));
    }
  }, [backend]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const doExport = async () => {
    setBusy(true);
    setNotice(null);
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({
        defaultPath: "conva.secrets.enc",
        filters: [{ name: "Encrypted secrets", extensions: ["enc"] }],
      });
      if (!dest) return;
      setNotice(await backend.secrets.export(dest));
      await refresh();
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doImport = async () => {
    setBusy(true);
    setNotice(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const src = await open({
        multiple: false,
        filters: [{ name: "Encrypted secrets", extensions: ["enc"] }],
      });
      if (!src || Array.isArray(src)) return;
      setNotice(await backend.secrets.import(src, false));
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <p className="mb-2 text-[11px] leading-relaxed text-fg-faint">
        Export your API keys to an encrypted file you can commit to git and pull
        on another computer. Unlocks from the{" "}
        <code className="font-mono">{status?.passphrase_env ?? "CONVA_SECRETS_PASSPHRASE"}</code>{" "}
        environment variable — set that once per machine and keys load on
        startup, no re-typing.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          disabled={busy || !status?.passphrase_set}
          onClick={() => void doExport()}
          className="btn"
        >
          Export encrypted…
        </button>
        <button
          type="button"
          disabled={busy || !status?.passphrase_set}
          onClick={() => void doImport()}
          className="btn"
        >
          Import…
        </button>
        <span className="text-[11px] text-fg-faint">
          {status?.passphrase_set
            ? status.file_present
              ? `Passphrase set · file present (${status.file_path})`
              : `Passphrase set · no file yet (${status?.file_path})`
            : "Set the passphrase env var to enable"}
        </span>
      </div>
      {notice && (
        <p className="mt-2 text-[11px] text-fg-muted" role="status">
          {notice}
        </p>
      )}
    </div>
  );
}

/** Device picker (A3). Selections persist immediately; a running session
 *  picks them up on the next start. */
/** Account sign-in (M0). OAuth via Supabase — identity only; audio,
 *  transcripts, and the library never leave the device. Tokens live in the OS
 *  keyring; the browser handles Google, so we never see IdP credentials. */
/** Official multi-colour Google "G". Rendered at the size Google's branding
 *  guidelines expect inside a sign-in button. */
function GoogleG({ size = 18 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 48 48"
      aria-hidden="true"
      className="shrink-0"
    >
      <path
        fill="#4285F4"
        d="M45.12 24.5c0-1.56-.14-3.06-.4-4.5H24v8.51h11.84c-.51 2.75-2.06 5.08-4.39 6.64v5.52h7.11c4.16-3.83 6.56-9.47 6.56-16.17z"
      />
      <path
        fill="#34A853"
        d="M24 46c5.94 0 10.92-1.97 14.56-5.33l-7.11-5.52c-1.97 1.32-4.49 2.1-7.45 2.1-5.73 0-10.58-3.87-12.31-9.07H4.34v5.7C7.96 41.07 15.4 46 24 46z"
      />
      <path
        fill="#FBBC05"
        d="M11.69 28.18C11.25 26.86 11 25.45 11 24s.25-2.86.69-4.18v-5.7H4.34C2.85 17.09 2 20.45 2 24s.85 6.91 2.34 9.88l7.35-5.7z"
      />
      <path
        fill="#EA4335"
        d="M24 10.75c3.23 0 6.13 1.11 8.41 3.29l6.31-6.31C34.91 4.18 29.93 2 24 2 15.4 2 7.96 6.93 4.34 14.12l7.35 5.7c1.73-5.2 6.58-9.07 12.31-9.07z"
      />
    </svg>
  );
}

function AccountSettings() {
  const backend = useBackend();
  const [status, setStatus] = useState<AuthStatus | null>(null);
  const [busy, setBusy] = useState(false);
  /** OAuth is out-of-band: the browser is open and we're waiting for the
   *  conva:// deep link to come back. Cleared by AUTH_CHANGED or cancel. */
  const [waiting, setWaiting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [mode, setMode] = useState<"signin" | "signup">("signin");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

  const refresh = useCallback(() => {
    void backend.auth
      .status()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, [backend]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // The OAuth result arrives as an event when the browser deep-links back
  // into the app (conva://auth/callback) — not as authStart's return value.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void backend
      .subscribe("authChanged", (payload) => {
        setWaiting(false);
        setBusy(false);
        if (payload.error) {
          setError(payload.error.replace(/^Error:\s*/, ""));
        } else if (payload.status) {
          setError(null);
          setStatus(payload.status);
        }
      })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [backend]);

  const signInGoogle = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await backend.auth.start("google");
      setWaiting(true); // browser is open; completion arrives via authChanged
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const cancelOauth = async () => {
    try {
      await backend.auth.cancel();
    } finally {
      setWaiting(false);
    }
  };

  const submitPassword = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!email.trim() || !password) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      if (mode === "signup") {
        setStatus(await backend.auth.signupPassword(email.trim(), password));
      } else {
        setStatus(await backend.auth.signinPassword(email.trim(), password));
      }
      setPassword("");
    } catch (err) {
      const msg = String(err);
      if (msg.includes("email_confirmation_required")) {
        setNotice("Account created. Check your email to confirm, then sign in.");
        setMode("signin");
        setPassword("");
      } else {
        setError(msg.replace(/^Error:\s*/, ""));
      }
    } finally {
      setBusy(false);
    }
  };

  const signOut = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await backend.auth.signout();
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // (Web sign-in is live — WebBackend.auth shares the getconva.com session —
  // so no isTauri() gate here; the PAL handles both surfaces.)
  if (status && !status.configured) {
    return <Notice>Sign-in isn’t configured in this build.</Notice>;
  }

  if (status?.signed_in) {
    return (
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-semibold text-fg">
            {status.email ?? "Signed in"}
          </p>
          <p className="text-[11px] text-fg-faint">Signed in to conva</p>
        </div>
        <button
          type="button"
          disabled={busy}
          onClick={() => void signOut()}
          className="btn shrink-0"
        >
          {busy ? "…" : "Sign out"}
        </button>
      </div>
    );
  }

  // Web: login is the website's job — the app never shows a sign-in form. (The
  // App guard already bounces unauthenticated web users to the website login;
  // this is the belt-and-suspenders fallback if they ever land here signed out.)
  if (isWeb) {
    return (
      <Notice>
        Sign in on the website to use conva.{" "}
        <a className="font-semibold text-fg underline" href="/login.html">
          Go to sign in →
        </a>
      </Notice>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-[11px] leading-relaxed text-fg-faint">
        Sign in to sync your preferences and manage your plan — the same account
        works on the website. Your audio, transcripts, and reference library stay
        on this device.
      </p>

      {/* Controls sit in a centered column sized to the widest one (Google),
          so nothing is full-bleed across a wide settings panel. */}
      <div className="mx-auto flex w-full max-w-[240px] flex-col gap-3">
        {/* Branded Google sign-in button (Google guidelines: white surface, the
            multi-colour G, "Sign in with Google"). */}
        <button
          type="button"
          disabled={busy || waiting}
          onClick={() => void signInGoogle()}
          className="flex h-8 w-full items-center justify-center gap-2.5 rounded-[4px] border border-[#dadce0] bg-white px-3 text-xs font-medium text-[#3c4043] shadow-sm transition hover:bg-[#f8f9fa] disabled:opacity-60 dark:border-[#5f6368] dark:bg-[#131314] dark:text-[#e3e3e3] dark:hover:bg-[#1e1f20]"
        >
          <GoogleG size={16} />
          {waiting
            ? "Waiting for the browser…"
            : busy
              ? "Opening browser…"
              : "Sign in with Google"}
        </button>
        {waiting && (
          <button
            type="button"
            onClick={() => void cancelOauth()}
            className="self-center text-[11px] text-fg-faint hover:underline"
          >
            Cancel sign-in
          </button>
        )}

        <div className="flex items-center gap-3 text-[10px] uppercase tracking-widest text-fg-faint">
          <span className="h-px flex-1 bg-border" />
          or
          <span className="h-px flex-1 bg-border" />
        </div>

        <form onSubmit={submitPassword} className="flex flex-col gap-2">
          <input
            type="email"
            autoComplete="email"
            required
            placeholder="you@example.com"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            className="input"
          />
          <input
            type="password"
            autoComplete={mode === "signup" ? "new-password" : "current-password"}
            required
            minLength={6}
            placeholder={
              mode === "signup" ? "Create a password (6+ chars)" : "Password"
            }
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="input"
          />
          {mode === "signin" && (
            <button
              type="button"
              onClick={() =>
                void backend.auth.openUrl("https://getconva.com/forgot-password")
              }
              className="self-end text-[11px] text-fg-faint hover:text-ai hover:underline"
            >
              Forgot password?
            </button>
          )}
          <button
            type="submit"
            disabled={busy}
            className="btn btn-primary h-8 w-full justify-center"
          >
            {busy
              ? "…"
              : mode === "signup"
                ? "Create account"
                : "Sign in with email"}
          </button>
        </form>

        <button
          type="button"
          onClick={() => {
            setMode((m) => (m === "signin" ? "signup" : "signin"));
            setError(null);
            setNotice(null);
          }}
          className="self-center text-[11px] text-ai hover:underline"
        >
          {mode === "signin"
            ? "New to conva? Create an account"
            : "Already have an account? Sign in"}
        </button>

        {notice && (
          <p className="text-center text-[11px] text-ai" role="status">
            {notice}
          </p>
        )}
        {error && (
          <p className="text-center text-[11px] text-rec" role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const config = useAppStore((s) => s.config);
  const updateConfig = useAppStore((s) => s.updateConfig);
  const refreshDevices = useAppStore((s) => s.refreshDevices);
  const deviceCount = useAppStore((s) => s.devices.length);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshedAt, setRefreshedAt] = useState<number | null>(null);
  if (!config) {
    // Web: no config store yet (`GET /v1/settings` is roadmap 1.3), and account
    // is the website's job — so no account here. App settings only.
    return (
      <ViewShell
        icon="settings"
        title="Settings"
        subtitle="App settings arrive with the hosted backend."
        actions={
          <button type="button" onClick={onClose} className="btn">
            Done
          </button>
        }
      >
        <Section title="Settings">
          <Notice>
            Device, transcription, and Ally-provider settings arrive with the
            hosted backend. Manage your account on the website.
          </Notice>
        </Section>
        <AboutSection />
      </ViewShell>
    );
  }

  const onRefresh = async () => {
    setRefreshing(true);
    try {
      await refreshDevices();
      setRefreshedAt(Date.now());
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <ViewShell
      icon="settings"
      title="Settings"
      subtitle="Devices, transcription, Ally providers, and portable config."
      actions={
        <button type="button" onClick={onClose} className="btn">
          Done
        </button>
      }
    >
      <Section
        title="Account"
        description="Sign in for settings sync and plan management. Identity only — no conversation content leaves your device."
      >
        <AccountSettings />
      </Section>

      <Section
        title="Audio devices"
        description="Device changes apply when the next session starts. Tip: use a headset — open speakers leak the other side into your microphone."
      >
        <div className="flex items-end gap-4">
          <DeviceSelect
            side="outbound"
            label="Microphone (you)"
            value={config.input_device}
            onChange={(d) => void updateConfig({ input_device: d })}
          />
          <DeviceSelect
            side="inbound"
            label="System audio to capture (them)"
            value={config.loopback_device}
            onChange={(d) => void updateConfig({ loopback_device: d })}
          />
          <button
            type="button"
            disabled={refreshing}
            onClick={() => void onRefresh()}
            title="Re-scan for audio devices you just plugged in"
            className="btn shrink-0"
          >
            {refreshing ? "Refreshing…" : "↻ Refresh devices"}
          </button>
        </div>
        <p className="mt-2 text-[11px] text-fg-faint" role="status">
          {refreshedAt
            ? `Rescanned — ${deviceCount} device(s) found.`
            : "Just plugged something in? Hit Refresh devices."}
        </p>
      </Section>

      <Section
        title="Transcription"
        description="Choose the speech engine and tune the speed/accuracy and noise trade-offs."
      >
        <div className="flex flex-col gap-3">
          <EngineSelect />
          <AsrModelSelect />
          <NoiseFilterControls />
        </div>
      </Section>

      <Section>
        <AllySettings />
      </Section>

      <Section
        title="Settings file"
        description={
          <>
            Defaults live in{" "}
            <code className="font-mono">conva.config.json</code> in the repo — a
            fresh machine starts from it. Export your tuned settings there and
            commit; API keys are never in this file.
          </>
        }
      >
        <ConfigFileControls />
      </Section>

      <Section title="Portable secrets">
        <SecretsSettings />
      </Section>

      <AboutSection />
    </ViewShell>
  );
}
