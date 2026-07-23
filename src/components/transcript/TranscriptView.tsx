import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import type { TranscriptSegment } from "@/lib/ipc";
import { useAppStore } from "@/state/app";
import { useAssistStore, type AssistCard } from "@/state/assist";
import { useTranscriptStore } from "@/state/transcript";

/** Stable identity for a transcript bubble (also the AI-card link key). */
function segmentKey(seg: TranscriptSegment): string {
  return `${seg.side}-${seg.seq}`;
}

function formatMs(ms: number): string {
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
}

function researchPrompt(text: string): string {
  return `Research this statement from the conversation. Give concise, immediately useful context — key facts, definitions, and anything I should know or verify: "${text}"`;
}

/** One SMS-style bubble: them left, you right. Final bubbles carry the ✦
 *  research action that sends this message to the AI (answer appears in the
 *  right column, linked back here). */
function Bubble({
  segment,
  registerEl,
  highlighted,
  onResearch,
  busy,
}: {
  segment: TranscriptSegment;
  registerEl: (key: string, el: HTMLElement | null) => void;
  highlighted: boolean;
  onResearch: (segment: TranscriptSegment) => void;
  busy: boolean;
}) {
  const inbound = segment.side === "inbound";
  const key = segmentKey(segment);
  return (
    <div
      className={`group flex w-full items-end gap-1.5 ${inbound ? "justify-start" : "justify-end"}`}
    >
      {/* Research affordance sits outside the bubble, on its outer edge. */}
      {!inbound && segment.is_final && (
        <ResearchButton
          onClick={() => onResearch(segment)}
          busy={busy}
          side="outbound"
        />
      )}
      <div
        ref={(el) => registerEl(key, el)}
        className={[
          "max-w-[78%] rounded-2xl border px-3 py-2 text-sm leading-relaxed",
          segment.is_final ? "segment-final" : "segment-partial border-dashed",
          inbound
            ? "rounded-bl-sm border-inbound/30 bg-inbound/10"
            : "rounded-br-sm border-outbound/30 bg-outbound/10",
          highlighted ? "ring-2 ring-ai/70" : "",
        ].join(" ")}
      >
        {segment.text}
        <div className="mt-1 flex items-center gap-2 font-mono text-[10px] text-fg-faint">
          <span>{inbound ? "Them" : "You"}</span>
          <span>{formatMs(segment.start_ms)}</span>
        </div>
      </div>
      {inbound && segment.is_final && (
        <ResearchButton
          onClick={() => onResearch(segment)}
          busy={busy}
          side="inbound"
        />
      )}
    </div>
  );
}

function ResearchButton({
  onClick,
  busy,
  side,
}: {
  onClick: () => void;
  busy: boolean;
  side: "inbound" | "outbound";
}) {
  return (
    <button
      type="button"
      disabled={busy}
      onClick={onClick}
      title="Research this message with AI"
      aria-label={`Research this ${side === "inbound" ? "received" : "sent"} message with AI`}
      className="mb-1 shrink-0 rounded-full border border-ai/40 px-1.5 py-0.5 text-[11px] text-ai opacity-0 transition-opacity hover:bg-ai/10 focus:opacity-100 group-hover:opacity-100 disabled:opacity-30"
    >
      ✦
    </button>
  );
}

const COLLAPSE_CHARS = 380;

/** AI answer card. Long answers collapse so the column stays scannable —
 *  "Show more" expands in place. Hovering highlights the source bubble. */
function AiCard({
  card,
  registerEl,
  onHover,
}: {
  card: AssistCard;
  registerEl: (id: string, el: HTMLElement | null) => void;
  onHover: (key: string | null) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const label =
    card.kind === "suggest_reply"
      ? "Suggested reply"
      : card.kind === "summarize"
        ? "Summary"
        : card.sourceQuote
          ? "Research"
          : (card.question ?? "Question");
  const long = card.text.length > COLLAPSE_CHARS;
  const shown =
    long && !expanded ? `${card.text.slice(0, COLLAPSE_CHARS)}…` : card.text;

  return (
    <div
      ref={(el) => registerEl(card.id, el)}
      onMouseEnter={() => onHover(card.sourceKey)}
      onMouseLeave={() => onHover(null)}
      className="rounded-md border border-ai/25 bg-ai/5 px-3 py-2"
    >
      <div className="mb-1 flex items-center gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-ai">
          {label}
        </span>
        {!card.done && (
          <span className="text-[11px] text-fg-faint" role="status">
            thinking…
          </span>
        )}
        <button
          type="button"
          onClick={() => void navigator.clipboard.writeText(card.text)}
          className="ml-auto text-[11px] text-fg-faint hover:text-fg"
        >
          Copy
        </button>
      </div>
      {card.sourceQuote && (
        <p className="mb-1 border-l-2 border-ai/40 pl-2 text-[11px] italic text-fg-muted">
          “
          {card.sourceQuote.length > 100
            ? `${card.sourceQuote.slice(0, 100)}…`
            : card.sourceQuote}
          ”
        </p>
      )}
      {card.error ? (
        <p className="text-xs text-rec">{card.error}</p>
      ) : (
        <p className="whitespace-pre-wrap text-sm leading-relaxed">
          {shown || "…"}
        </p>
      )}
      {long && (
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="mt-1 text-[11px] font-semibold text-ai hover:underline"
        >
          {expanded ? "Show less" : "Show more"}
        </button>
      )}
      {card.sources.length > 0 && (
        <p className="mt-1.5 text-[11px] text-fg-faint">
          sources:{" "}
          {[...new Set(card.sources.map((s) => `${s.file_name} · ${s.location}`))]
            .slice(0, 3)
            .join("  ·  ")}
        </p>
      )}
    </div>
  );
}

interface Link {
  cardId: string;
  sourceKey: string;
}

/** Connector lines from each AI card to the bubble it researched. Drawn in
 *  an overlay SVG; recomputed on scroll/resize/content changes. */
function Connectors({
  container,
  links,
  bubbleEls,
  cardEls,
  activeKey,
  tick,
}: {
  container: HTMLElement | null;
  links: Link[];
  bubbleEls: Map<string, HTMLElement>;
  cardEls: Map<string, HTMLElement>;
  activeKey: string | null;
  tick: number;
}) {
  const [paths, setPaths] = useState<
    Array<{ d: string; active: boolean; key: string }>
  >([]);

  useLayoutEffect(() => {
    if (!container) return;
    const base = container.getBoundingClientRect();
    const next: Array<{ d: string; active: boolean; key: string }> = [];
    for (const link of links) {
      const bubble = bubbleEls.get(link.sourceKey);
      const card = cardEls.get(link.cardId);
      if (!bubble || !card) continue;
      const b = bubble.getBoundingClientRect();
      const c = card.getBoundingClientRect();
      const x1 = b.right - base.left;
      const y1 = b.top + b.height / 2 - base.top;
      const x2 = c.left - base.left;
      const y2 = c.top + c.height / 2 - base.top;
      const mid = (x1 + x2) / 2;
      next.push({
        d: `M ${x1} ${y1} C ${mid} ${y1}, ${mid} ${y2}, ${x2} ${y2}`,
        active: activeKey === link.sourceKey,
        key: `${link.cardId}-${link.sourceKey}`,
      });
    }
    setPaths(next);
  }, [container, links, bubbleEls, cardEls, activeKey, tick]);

  return (
    <svg
      className="pointer-events-none absolute inset-0 h-full w-full"
      aria-hidden
    >
      {paths.map((p) => (
        <path
          key={p.key}
          d={p.d}
          fill="none"
          className={p.active ? "stroke-ai" : "stroke-ai/35"}
          strokeWidth={p.active ? 2 : 1.25}
          strokeDasharray={p.active ? "none" : "4 3"}
        />
      ))}
    </svg>
  );
}

function useAutoScroll(dep: unknown) {
  const ref = useRef<HTMLDivElement>(null);
  const [pinned, setPinned] = useState(true);
  useEffect(() => {
    if (pinned && ref.current) {
      ref.current.scrollTop = ref.current.scrollHeight;
    }
  }, [pinned, dep]);
  const onScroll = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    setPinned(el.scrollHeight - el.scrollTop - el.clientHeight < 48);
  }, []);
  return { ref, pinned, setPinned, onScroll };
}

/** Sidecar fallback: single merged feed that fits a 380 px strip. */
function SidecarFeed({ segments }: { segments: TranscriptSegment[] }) {
  const merged = [...segments].sort((a, b) => a.start_ms - b.start_ms);
  const { ref, pinned, setPinned, onScroll } = useAutoScroll(
    merged[merged.length - 1],
  );
  const noop = useCallback(() => {}, []);
  const request = useAssistStore((s) => s.request);
  const busy = useAssistStore((s) => s.busy);
  return (
    <main className="relative flex min-h-0 flex-1 flex-col">
      <div
        ref={ref}
        onScroll={onScroll}
        className="flex flex-1 flex-col gap-2 overflow-y-auto px-3 pb-4 pt-2"
        role="log"
        aria-live="polite"
        aria-label="Conversation transcript"
      >
        {merged.length === 0 ? (
          <p className="mt-8 text-center text-xs text-fg-faint">
            Both sides of the conversation appear here.
          </p>
        ) : (
          merged.map((seg) => (
            <Bubble
              key={segmentKey(seg)}
              segment={seg}
              registerEl={noop}
              highlighted={false}
              busy={busy}
              onResearch={(s) =>
                void request("question", researchPrompt(s.text), {
                  key: segmentKey(s),
                  quote: s.text,
                })
              }
            />
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
    </main>
  );
}

/**
 * Conversation workspace: left — the live conversation as SMS-style bubbles
 * (them left, you right) with a per-message ✦ research action; right — the
 * AI output column (research answers, suggested replies, summaries), long
 * answers collapsed. Connector lines tie each research answer back to its
 * bubble. Sidecar mode (U9) keeps the single merged feed.
 */
export function TranscriptView() {
  const liveSegments = useTranscriptStore((s) => s.segments);
  const archived = useTranscriptStore((s) => s.archived);
  const sidecar = useAppStore((s) => s.sidecar);
  const cards = useAssistStore((s) => s.cards);
  const busy = useAssistStore((s) => s.busy);
  const request = useAssistStore((s) => s.request);

  const containerRef = useRef<HTMLDivElement>(null);
  const bubbleEls = useRef(new Map<string, HTMLElement>());
  const cardEls = useRef(new Map<string, HTMLElement>());
  const [activeKey, setActiveKey] = useState<string | null>(null);
  // Bumped on scroll/resize so connector geometry follows the content.
  const [tick, setTick] = useState(0);
  const bump = useCallback(() => setTick((t) => t + 1), []);

  // Archived material (earlier runs of the open conversation / a loaded
  // conversation) renders above the live run, which is merged by time.
  const merged = [
    ...archived,
    ...[...liveSegments].sort((a, b) => a.start_ms - b.start_ms),
  ];
  const convo = useAutoScroll(merged[merged.length - 1]);
  const aiCol = useAutoScroll(null);

  useEffect(() => {
    window.addEventListener("resize", bump);
    return () => window.removeEventListener("resize", bump);
  }, [bump]);

  const registerBubble = useCallback((key: string, el: HTMLElement | null) => {
    if (el) bubbleEls.current.set(key, el);
    else bubbleEls.current.delete(key);
  }, []);
  const registerCard = useCallback((id: string, el: HTMLElement | null) => {
    if (el) cardEls.current.set(id, el);
    else cardEls.current.delete(id);
  }, []);

  if (sidecar) {
    return <SidecarFeed segments={merged} />;
  }

  const links: Link[] = cards
    .filter((c): c is AssistCard & { sourceKey: string } => !!c.sourceKey)
    .map((c) => ({ cardId: c.id, sourceKey: c.sourceKey }));

  const research = (seg: TranscriptSegment) =>
    void request("question", researchPrompt(seg.text), {
      key: segmentKey(seg),
      quote: seg.text,
    });

  return (
    <main ref={containerRef} className="relative flex min-h-0 min-w-0 flex-1">
      {/* Conversation — left */}
      <section className="relative flex min-w-0 flex-[3] flex-col">
        <h2 className="shrink-0 px-4 py-2 text-xs font-semibold uppercase tracking-widest text-fg-muted">
          Conversation
        </h2>
        <div
          ref={convo.ref}
          onScroll={() => {
            convo.onScroll();
            bump();
          }}
          className="flex flex-1 flex-col gap-2 overflow-y-auto px-4 pb-4"
          role="log"
          aria-live="polite"
          aria-label="Conversation transcript"
        >
          {merged.length === 0 ? (
            <p className="mt-8 text-center text-xs text-fg-faint">
              The conversation appears here — them on the left, you on the
              right. Hover a message and press ✦ to have AI research it.
            </p>
          ) : (
            merged.map((seg) => (
              <Bubble
                key={segmentKey(seg)}
                segment={seg}
                registerEl={registerBubble}
                highlighted={activeKey === segmentKey(seg)}
                onResearch={research}
                busy={busy}
              />
            ))
          )}
        </div>
        {!convo.pinned && (
          <button
            type="button"
            onClick={() => convo.setPinned(true)}
            className="absolute bottom-3 left-1/2 -translate-x-1/2 rounded-full border border-border bg-panel px-3 py-1 text-[11px] text-fg-muted shadow hover:text-fg"
          >
            ↓ Jump to live
          </button>
        )}
      </section>

      <div className="w-px shrink-0 bg-border" aria-hidden />

      {/* AI output — right */}
      <section className="flex min-w-0 flex-[2] flex-col">
        <h2 className="shrink-0 px-4 py-2 text-xs font-semibold uppercase tracking-widest text-ai">
          ✦ AI
        </h2>
        <div
          ref={aiCol.ref}
          onScroll={() => {
            aiCol.onScroll();
            bump();
          }}
          className="flex flex-1 flex-col gap-2 overflow-y-auto px-4 pb-4"
          aria-label="AI output"
        >
          {cards.length === 0 ? (
            <p className="mt-8 text-center text-xs text-fg-faint">
              AI output lands here — press ✦ on any message, or use Suggest
              reply / Summarize below.
            </p>
          ) : (
            cards.map((card) => (
              <AiCard
                key={card.id}
                card={card}
                registerEl={registerCard}
                onHover={setActiveKey}
              />
            ))
          )}
        </div>
      </section>

      <Connectors
        container={containerRef.current}
        links={links}
        bubbleEls={bubbleEls.current}
        cardEls={cardEls.current}
        activeKey={activeKey}
        tick={tick + cards.length + merged.length}
      />
    </main>
  );
}
