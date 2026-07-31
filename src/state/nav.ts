import { create } from "zustand";

/**
 * Studio navigation state (UI overhaul M2). The app is a single instrument
 * with a left icon rail selecting the active {@link View}; "live" is the
 * transcript/Ally cockpit, the rest are the former dropdown panels promoted
 * to first-class routed views. The ⌘K command palette floats over any view.
 */
export type View =
  | "dashboard"
  | "live"
  | "library"
  | "sessions"
  | "conversations"
  | "features"
  | "whatsnew"
  | "releases"
  | "settings"
  | "profile";

interface NavState {
  view: View;
  setView: (view: View) => void;

  /** ⌘K command palette visibility. */
  paletteOpen: boolean;
  openPalette: () => void;
  closePalette: () => void;
  togglePalette: () => void;
}

export const useNavStore = create<NavState>((set) => ({
  view: "dashboard",
  setView: (view) => set({ view, paletteOpen: false }),

  paletteOpen: false,
  openPalette: () => set({ paletteOpen: true }),
  closePalette: () => set({ paletteOpen: false }),
  togglePalette: () => set((s) => ({ paletteOpen: !s.paletteOpen })),
}));
