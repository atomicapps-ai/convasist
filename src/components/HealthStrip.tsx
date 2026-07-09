import type { AudioLevelEvent, StreamSide } from "@/lib/ipc";
import { useAppStore } from "@/state/app";
import { useTranscriptStore } from "@/state/transcript";

/** Map dBFS [-60, 0] onto a 0–100% meter width. */
function levelPercent(dbfs: number): number {
  return Math.max(0, Math.min(100, ((dbfs + 60) / 60) * 100));
}

function Meter({
  side,
  label,
  level,
}: {
  side: StreamSide;
  label: string;
  level: AudioLevelEvent | null;
}) {
  const accent = side === "inbound" ? "text-inbound" : "text-outbound";
  const bar = side === "inbound" ? "bg-inbound" : "bg-outbound";
  const stalled = level !== null && !level.healthy;

  return (
    <span className={`flex items-center gap-1.5 font-mono text-[11px] ${accent}`}>
      <span aria-hidden>{side === "inbound" ? "🔊" : "🎙"}</span>
      {label}
      <span
        className="h-1.5 w-16 overflow-hidden rounded-full bg-border"
        role="meter"
        aria-label={`${label} level`}
        aria-valuemin={-60}
        aria-valuemax={0}
        aria-valuenow={level ? Math.round(level.rms_dbfs) : -60}
      >
        <span
          className={`block h-full ${stalled ? "bg-rec" : bar}`}
          style={{ width: `${level ? levelPercent(level.rms_dbfs) : 0}%` }}
        />
      </span>
      {stalled ? (
        <span className="text-rec">stalled</span>
      ) : (
        <span className="text-fg-faint">
          {level ? `${level.rms_dbfs.toFixed(0)} dB` : "— dB"}
        </span>
      )}
    </span>
  );
}

/** Bottom health strip: VU meters + stall warnings (design §5.2, A4). */
export function HealthStrip() {
  const levels = useTranscriptStore((s) => s.levels);
  const segments = useTranscriptStore((s) => s.segments);
  const config = useAppStore((s) => s.config);

  // Latency readout (U10 lite): last decode + rolling average of the last
  // 10 finalized segments.
  const finals = segments.filter((s) => s.is_final).slice(-10);
  const lastLatency = segments[segments.length - 1]?.latency_ms;
  const avgLatency =
    finals.length > 0
      ? Math.round(
          finals.reduce((sum, s) => sum + s.latency_ms, 0) / finals.length,
        )
      : undefined;

  return (
    <footer className="flex h-8 shrink-0 items-center gap-4 border-t border-border bg-panel px-4">
      <Meter side="outbound" label="mic" level={levels.outbound} />
      <Meter side="inbound" label="system" level={levels.inbound} />
      <span className="ml-auto font-mono text-[11px] text-fg-faint">
        whisper {config?.whisper_model ?? "…"}
        {lastLatency !== undefined ? ` · ${lastLatency}ms` : ""}
        {avgLatency !== undefined ? ` · avg ${avgLatency}ms` : ""}
      </span>
    </footer>
  );
}
