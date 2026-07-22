import { useCallback, useEffect, useState } from "react";

import { AiSettings } from "@/components/AiSettings";
import {
  deepgramKeyStatus,
  exportConfig,
  importConfig,
  listWhisperModels,
  secretsExport,
  secretsImport,
  secretsStatus,
  setDeepgramKey,
} from "@/lib/commands";
import type { SecretsStatus, StreamSide, WhisperModelInfo } from "@/lib/ipc";
import { isTauri } from "@/lib/ipc";
import { useAppStore } from "@/state/app";

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
    <label className="flex min-w-0 flex-1 flex-col gap-1 text-xs text-fg-muted">
      {label}
      <select
        className="rounded-md border border-border bg-bg px-2 py-1.5 text-xs text-fg"
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
  const config = useAppStore((s) => s.config);
  const updateConfig = useAppStore((s) => s.updateConfig);
  const [models, setModels] = useState<WhisperModelInfo[]>([]);

  useEffect(() => {
    if (!isTauri()) return;
    void listWhisperModels()
      .then(setModels)
      .catch(() => setModels([]));
  }, []);

  if (!config) return null;
  const current = models.find((m) => m.id === config.whisper_model);

  return (
    <div className="mt-3 flex items-end gap-3">
      <label className="flex min-w-0 flex-col gap-1 text-xs text-fg-muted">
        Speech-to-text model (speed vs. accuracy)
        <select
          className="rounded-md border border-border bg-bg px-2 py-1.5 text-xs text-fg"
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
  const config = useAppStore((s) => s.config);
  const updateConfig = useAppStore((s) => s.updateConfig);
  const [hasKey, setHasKey] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    void deepgramKeyStatus().then(setHasKey).catch(() => {});
  }, []);

  if (!config) return null;
  const cloud = config.asr_engine === "deepgram_cloud";

  const saveKey = async () => {
    const next = keyDraft.trim();
    try {
      await setDeepgramKey(next);
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
    <div className="mt-3 rounded-md border border-border bg-bg px-3 py-2">
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
          className="min-w-0 flex-1 rounded-md border border-border bg-panel px-2 py-1 text-xs text-fg placeholder:text-fg-faint"
        />
        <button
          type="button"
          onClick={() => void saveKey()}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg"
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
    <div className="mt-3">
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

/** Config file sync: settings live in `convasist.config.json`, committed to
 *  the repo — a fresh machine seeds from it automatically; these buttons
 *  push/pull your current settings to/from that file. Keys are NOT in this
 *  file (they use the encrypted secrets flow below). */
function ConfigFileControls() {
  const updateConfig = useAppStore((s) => s.updateConfig);
  const [notice, setNotice] = useState<string | null>(null);

  const doExport = async () => {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({
        defaultPath: "convasist.config.json",
        filters: [{ name: "Config", extensions: ["json"] }],
      });
      if (!dest) return;
      await exportConfig(dest);
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
      const config = await importConfig(src);
      // Push the imported values through the store so the UI reflects them.
      await updateConfig(config);
      setNotice("Config applied.");
    } catch (e) {
      setNotice(String(e));
    }
  };

  return (
    <div className="mt-4 border-t border-border pt-3">
      <h3 className="text-xs font-semibold uppercase tracking-wider text-fg-muted">
        Settings file
      </h3>
      <p className="mt-1 text-[11px] text-fg-faint">
        Defaults live in <code className="font-mono">convasist.config.json</code>{" "}
        in the repo — a fresh machine starts from it. Export your tuned
        settings there and commit; API keys are never in this file.
      </p>
      <div className="mt-2 flex items-center gap-2">
        <button
          type="button"
          onClick={() => void doExport()}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg"
        >
          Export settings…
        </button>
        <button
          type="button"
          onClick={() => void doImport()}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg"
        >
          Import…
        </button>
        {notice && (
          <span className="text-[11px] text-fg-faint" role="status">
            {notice}
          </span>
        )}
      </div>
    </div>
  );
}

/** Portable encrypted secrets: export API keys to a git-committable file and
 *  load them on another machine. The passphrase comes from an env var, so the
 *  file is safe to commit and keys never re-typed per launch. */
function SecretsSettings() {
  const [status, setStatus] = useState<SecretsStatus | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setStatus(await secretsStatus());
    } catch (e) {
      setNotice(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const doExport = async () => {
    setBusy(true);
    setNotice(null);
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({
        defaultPath: "convasist.secrets.enc",
        filters: [{ name: "Encrypted secrets", extensions: ["enc"] }],
      });
      if (!dest) return;
      setNotice(await secretsExport(dest));
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
      setNotice(await secretsImport(src, false));
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mt-4 border-t border-border pt-3">
      <h3 className="text-xs font-semibold uppercase tracking-wider text-fg-muted">
        Portable secrets
      </h3>
      <p className="mt-1 text-[11px] text-fg-faint">
        Export your API keys to an encrypted file you can commit to git and pull
        on another computer. Unlocks from the{" "}
        <code className="font-mono">{status?.passphrase_env ?? "CONVASIST_SECRETS_PASSPHRASE"}</code>{" "}
        environment variable — set that once per machine and keys load on
        startup, no re-typing.
      </p>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <button
          type="button"
          disabled={busy || !status?.passphrase_set}
          onClick={() => void doExport()}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
        >
          Export encrypted…
        </button>
        <button
          type="button"
          disabled={busy || !status?.passphrase_set}
          onClick={() => void doImport()}
          className="rounded-md border border-border px-2.5 py-1 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
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
export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const config = useAppStore((s) => s.config);
  const updateConfig = useAppStore((s) => s.updateConfig);
  const refreshDevices = useAppStore((s) => s.refreshDevices);
  const deviceCount = useAppStore((s) => s.devices.length);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshedAt, setRefreshedAt] = useState<number | null>(null);
  if (!config) return null;

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
    <div className="border-b border-border bg-panel px-4 py-3">
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
          className="rounded-md border border-border px-3 py-1.5 text-xs text-fg-muted hover:text-fg disabled:opacity-50"
        >
          {refreshing ? "Refreshing…" : "↻ Refresh devices"}
        </button>
        <button
          type="button"
          onClick={onClose}
          className="rounded-md border border-border px-3 py-1.5 text-xs text-fg-muted hover:text-fg"
        >
          Close
        </button>
      </div>
      <p className="mt-2 text-[11px] text-fg-faint" role="status">
        {refreshedAt
          ? `Rescanned — ${deviceCount} device(s) found. `
          : "Just plugged something in? Hit Refresh devices. "}
        Device changes apply when the next session starts. Tip: use a headset —
        open speakers leak the other side into your microphone.
      </p>
      <EngineSelect />
      <AsrModelSelect />
      <NoiseFilterControls />
      <AiSettings />
      <ConfigFileControls />
      <SecretsSettings />
    </div>
  );
}
