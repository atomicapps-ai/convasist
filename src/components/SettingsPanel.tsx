import { AiSettings } from "@/components/AiSettings";
import type { StreamSide } from "@/lib/ipc";
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

/** Device picker (A3). Selections persist immediately; a running session
 *  picks them up on the next start. */
export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const config = useAppStore((s) => s.config);
  const updateConfig = useAppStore((s) => s.updateConfig);
  if (!config) return null;

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
          onClick={onClose}
          className="rounded-md border border-border px-3 py-1.5 text-xs text-fg-muted hover:text-fg"
        >
          Close
        </button>
      </div>
      <p className="mt-2 text-[11px] text-fg-faint">
        Device changes apply when the next session starts. Tip: use a headset —
        open speakers leak the other side into your microphone.
      </p>
      <AiSettings />
    </div>
  );
}
