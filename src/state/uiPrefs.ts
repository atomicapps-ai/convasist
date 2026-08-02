import { create } from "zustand";

/**
 * Small, local UI preferences for the Ally column — persisted to localStorage
 * (not synced settings). Font size for the research text and whether the
 * collapsible reasoning block starts open.
 */
const FONT_KEY = "conva.ally.fontPx";
const REASONING_KEY = "conva.ally.reasoningOpen";
const FONT_MIN = 11;
const FONT_MAX = 20;
const FONT_DEFAULT = 14;

function loadFont(): number {
  const v = Number(localStorage.getItem(FONT_KEY));
  return v >= FONT_MIN && v <= FONT_MAX ? v : FONT_DEFAULT;
}

interface UiPrefs {
  /** Ally research text size, in px. */
  allyFontPx: number;
  /** Whether the reasoning ("thinking") block starts expanded. */
  reasoningDefaultOpen: boolean;
  setAllyFontPx: (px: number) => void;
  bumpAllyFont: (delta: number) => void;
  setReasoningDefaultOpen: (open: boolean) => void;
}

export const useUiPrefs = create<UiPrefs>((set) => ({
  allyFontPx: loadFont(),
  reasoningDefaultOpen: localStorage.getItem(REASONING_KEY) === "1",

  setAllyFontPx: (px) => {
    const clamped = Math.max(FONT_MIN, Math.min(FONT_MAX, Math.round(px)));
    localStorage.setItem(FONT_KEY, String(clamped));
    set({ allyFontPx: clamped });
  },
  bumpAllyFont: (delta) =>
    set((s) => {
      const clamped = Math.max(FONT_MIN, Math.min(FONT_MAX, s.allyFontPx + delta));
      localStorage.setItem(FONT_KEY, String(clamped));
      return { allyFontPx: clamped };
    }),
  setReasoningDefaultOpen: (open) => {
    localStorage.setItem(REASONING_KEY, open ? "1" : "0");
    set({ reasoningDefaultOpen: open });
  },
}));

export const ALLY_FONT_MIN = FONT_MIN;
export const ALLY_FONT_MAX = FONT_MAX;
