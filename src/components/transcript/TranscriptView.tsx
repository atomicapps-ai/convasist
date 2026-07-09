import { useEffect, useRef, useState } from "react";

import type { TranscriptSegment } from "@/lib/ipc";
import { useTranscriptStore } from "@/state/transcript";

function SegmentBubble({ segment }: { segment: TranscriptSegment }) {
  const inbound = segment.side === "inbound";
  return (
    <div
      className={[
        "max-w-[85%] rounded-lg border px-3 py-2 text-sm leading-relaxed",
        segment.is_final ? "segment-final" : "segment-partial border-dashed",
        inbound
          ? "self-start border-inbound/30 bg-inbound/5"
          : "self-end border-outbound/30 bg-outbound/5",
      ].join(" ")}
    >
      {segment.text}
      <div className="mt-1 font-mono text-[10px] text-fg-faint">
        {formatMs(segment.start_ms)}
      </div>
    </div>
  );
}

function formatMs(ms: number): string {
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
}

function Column({
  title,
  accentClass,
  segments,
  emptyHint,
}: {
  title: string;
  accentClass: string;
  segments: TranscriptSegment[];
  emptyHint: string;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  // Smart auto-scroll (U2): follow the live edge unless the user scrolled up.
  const [pinned, setPinned] = useState(true);
  const lastSegment = segments[segments.length - 1];

  useEffect(() => {
    if (pinned && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [pinned, lastSegment]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    setPinned(atBottom);
  };

  return (
    <section className="relative flex min-w-0 flex-1 flex-col">
      <h2
        className={`shrink-0 px-4 py-2 text-xs font-semibold uppercase tracking-widest ${accentClass}`}
      >
        {title}
      </h2>
      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="flex flex-1 flex-col gap-2 overflow-y-auto px-4 pb-4"
        role="log"
        aria-live="polite"
        aria-label={`${title} transcript`}
      >
        {segments.length === 0 ? (
          <p className="mt-8 text-center text-xs text-fg-faint">{emptyHint}</p>
        ) : (
          segments.map((seg) => (
            <SegmentBubble key={`${seg.side}-${seg.seq}`} segment={seg} />
          ))
        )}
      </div>
      {!pinned && (
        <button
          type="button"
          onClick={() => setPinned(true)}
          className="absolute bottom-3 left-1/2 -translate-x-1/2 rounded-full border border-border bg-panel px-3 py-1 text-[11px] text-fg-muted shadow hover:text-fg"
        >
          ↓ Jump to live
        </button>
      )}
    </section>
  );
}

/** Dual-column live transcript: THEM (inbound) left, YOU (outbound) right. */
export function TranscriptView() {
  const segments = useTranscriptStore((s) => s.segments);
  const inbound = segments.filter((s) => s.side === "inbound");
  const outbound = segments.filter((s) => s.side === "outbound");

  return (
    <main className="flex min-h-0 flex-1">
      <Column
        title="Them · system audio"
        accentClass="text-inbound"
        segments={inbound}
        emptyHint="What you hear (calls, videos, any app audio) is transcribed here."
      />
      <div className="w-px shrink-0 bg-border" aria-hidden />
      <Column
        title="You · microphone"
        accentClass="text-outbound"
        segments={outbound}
        emptyHint="What you say into the microphone is transcribed here."
      />
    </main>
  );
}
