import { useCallback, useEffect, useState } from "react";

import { AiSettings } from "@/components/AiSettings";
import { secretsExport, secretsImport, secretsStatus } from "@/lib/commands";
import type { SecretsStatus, StreamSide } from "@/lib/ipc";
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
      <AiSettings />
      <SecretsSettings />
    </div>
  );
}
