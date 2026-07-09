import type { AudioLevelEvent, StreamSide } from "@/lib/ipc";
import { useTranscriptStore } from "@/state/transcript";

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
  return (
    <span className={`flex items-center gap-1.5 font-mono text-[11px] ${accent}`}>
      <span aria-hidden>{side === "inbound" ? "🔊" : "🎙"}</span>
      {label}
      <span className="text-fg-faint">
        {level ? `${level.rms_dbfs.toFixed(0)} dB` : "— dB"}
      </span>
    </span>
  );
}

/** Bottom health strip: VU meters + pipeline status (design §5.2, A4). */
export function HealthStrip() {
  const levels = useTranscriptStore((s) => s.levels);
  return (
    <footer className="flex h-8 shrink-0 items-center gap-4 border-t border-border bg-panel px-4">
      <Meter side="outbound" label="mic" level={levels.outbound} />
      <Meter side="inbound" label="system" level={levels.inbound} />
      <span className="ml-auto font-mono text-[11px] text-fg-faint">
        capture M1 · asr M2
      </span>
    </footer>
  );
}
