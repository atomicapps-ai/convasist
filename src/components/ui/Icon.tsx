/**
 * conva icon set — hand-drawn inline SVG line icons.
 *
 * Dependency-free (no icon package) so nothing has to reach the network under
 * the app's locked-down CSP, and every glyph shares one stroke language:
 * 24×24 grid, 1.6 stroke, round caps/joins, `currentColor` so callers tint
 * with text-* utilities. Matches the inline-SVG idiom already used by <Core>.
 */

import type { ReactNode } from "react";

export type IconName =
  | "live"
  | "library"
  | "sessions"
  | "conversations"
  | "settings"
  | "record"
  | "compact"
  | "search"
  | "command"
  | "close"
  | "chevron"
  | "unfoldMore"
  | "unfoldLess"
  | "mic"
  | "system"
  | "account"
  | "expand"
  | "lightbulb"
  | "book"
  | "howto"
  | "elaborate"
  | "simicon"
  | "edit"
  | "reasoning"
  | "summarize"
  | "more"
  | "trash";

const PATHS: Record<IconName, ReactNode> = {
  // Live cockpit — a sound/signal waveform.
  live: (
    <>
      <path d="M4 12h2l1.5-5 3 12L13 5l2 9 1.5-2H20" />
    </>
  ),
  // Reference library — stacked documents.
  library: (
    <>
      <path d="M5 4.5h9l3 3V19.5H5z" />
      <path d="M13.5 4.5v3.5H17" />
      <path d="M8 12h6M8 15h6" />
    </>
  ),
  // Simicon — the icon for Sim Con (Simulated Conversation): an AI persona
  // (person + sparkle), the AI-generated counterparty.
  simicon: (
    <>
      <circle cx="12" cy="8" r="3" />
      <path d="M6 19a6 6 0 0 1 12 0" />
      <path d="M18.6 4.2l.55 1.5 1.5.55-1.5.55-.55 1.5-.55-1.5-1.5-.55 1.5-.55z" />
    </>
  ),
  // Edit — a pencil.
  edit: (
    <>
      <path d="M16.5 3.5l4 4L8 20l-4.5 1 1-4.5z" />
      <path d="M14 6l4 4" />
    </>
  ),
  // Trash — delete.
  trash: (
    <>
      <path d="M4 7h16" />
      <path d="M9 7V5.2A1.7 1.7 0 0 1 10.7 3.5h2.6A1.7 1.7 0 0 1 15 5.2V7" />
      <path d="M6 7l1 12.3A1.7 1.7 0 0 0 8.7 21h6.6A1.7 1.7 0 0 0 17 19.3L18 7" />
      <path d="M10 11v6M14 11v6" />
    </>
  ),
  // Summarize — condensed lines (a short recap).
  summarize: (
    <>
      <path d="M4 6h16M4 10h16M4 14h11M4 18h7" />
    </>
  ),
  // Overflow menu — kebab (three vertical dots).
  more: (
    <>
      <circle cx="12" cy="5" r="1.1" />
      <circle cx="12" cy="12" r="1.1" />
      <circle cx="12" cy="19" r="1.1" />
    </>
  ),
  // Reasoning / "thinking" — a thought bubble with an ellipsis (details behind).
  reasoning: (
    <>
      <path d="M5 5.5h11a3 3 0 0 1 3 3v3a3 3 0 0 1-3 3h-5.5L7 18v-3.5H5a3 3 0 0 1-3-3v-3a3 3 0 0 1 3-3z" />
      <circle cx="8" cy="10" r="0.9" />
      <circle cx="11.5" cy="10" r="0.9" />
      <circle cx="15" cy="10" r="0.9" />
    </>
  ),
  // Sessions — clock / history.
  sessions: (
    <>
      <circle cx="12" cy="12" r="7.5" />
      <path d="M12 8v4.3l3 1.8" />
    </>
  ),
  // Conversations — two chat bubbles.
  conversations: (
    <>
      <path d="M4 6.5h10v7H8l-3 2.5v-2.5H4z" />
      <path d="M17 9.5h3v6h-1v2l-2.2-2H12.5" />
    </>
  ),
  // Settings — sliders.
  settings: (
    <>
      <path d="M4 8h9M17 8h3M4 16h3M11 16h9" />
      <circle cx="15" cy="8" r="2" />
      <circle cx="9" cy="16" r="2" />
    </>
  ),
  record: <circle cx="12" cy="12" r="5" fill="currentColor" stroke="none" />,
  // Compact — a docked right panel.
  compact: (
    <>
      <rect x="4" y="5" width="16" height="14" rx="2" />
      <path d="M14 5v14" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="6" />
      <path d="M20 20l-4.5-4.5" />
    </>
  ),
  // The ⌘ command glyph.
  command: (
    <path d="M9 9V7.5A2.5 2.5 0 1 0 6.5 10H9m0 0v5m0-5h6m0 0V7.5A2.5 2.5 0 1 1 17.5 10H15m0 0v5m0 0v1.5A2.5 2.5 0 1 0 17.5 14H15m0 0H9m0 0v1.5A2.5 2.5 0 1 1 6.5 14H9" />
  ),
  close: <path d="M6 6l12 12M18 6L6 18" />,
  // Single chevron (points down; rotate via CSS for an expand/collapse caret).
  chevron: <path d="M6 9l6 6 6-6" />,
  // Expand-all — chevrons pointing apart.
  unfoldMore: <path d="M8 9l4-4 4 4M8 15l4 4 4-4" />,
  // Collapse-all — chevrons pointing together.
  unfoldLess: <path d="M8 5l4 4 4-4M8 19l4-4 4 4" />,
  // Microphone — the "you" stream.
  mic: (
    <>
      <path d="M12 4a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V7a3 3 0 0 1 3-3z" />
      <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
    </>
  ),
  // System audio bars — the "them" stream.
  system: <path d="M5 9v6M9 5v14M15 7v10M19 10v4" />,
  // Account — user head + shoulders.
  account: (
    <>
      <circle cx="12" cy="8" r="3.5" />
      <path d="M5.5 19.5a6.5 6.5 0 0 1 13 0" />
    </>
  ),
  // Expand — four corner arrows (leave compact / go full size).
  expand: <path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5" />,
  // Ask Ally — a lightbulb with a lightning filament (initiates AI).
  lightbulb: (
    <>
      <path d="M12 3a6 6 0 0 0-3.6 10.8c.6.45 1.1 1.15 1.1 1.95V17h5v-1.25c0-.8.5-1.5 1.1-1.95A6 6 0 0 0 12 3z" />
      <path d="M9.7 20h4.6M10.7 22.2h2.6" />
      <path d="M12.7 7.2l-2.2 3.7h2.5L11 15" />
    </>
  ),
  // Definition — an open book.
  book: (
    <>
      <path d="M12 6.5C10.5 5 8 4.5 4 4.8v12.4c4-.3 6.5.2 8 1.7 1.5-1.5 4-2 8-1.7V4.8c-4-.3-6.5.2-8 1.7z" />
      <path d="M12 6.5v12.1" />
    </>
  ),
  // How-to — a question mark in a rounded square.
  howto: (
    <>
      <rect x="4" y="4" width="16" height="16" rx="4" />
      <path d="M9.5 9.2a2.6 2.6 0 0 1 5 .9c0 1.7-2.5 2-2.5 3.6" />
      <path d="M12 17h.01" />
    </>
  ),
  // Elaborate — expand outward (plus with radiating arrows).
  elaborate: (
    <>
      <path d="M12 8v8M8 12h8" />
      <path d="M5 5l2 2M19 5l-2 2M5 19l2-2M19 19l-2-2" />
    </>
  ),
};

export function Icon({
  name,
  size = 20,
  className = "",
  strokeWidth = 1.6,
}: {
  name: IconName;
  size?: number;
  className?: string;
  strokeWidth?: number;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {PATHS[name]}
    </svg>
  );
}
