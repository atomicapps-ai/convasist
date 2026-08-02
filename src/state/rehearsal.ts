import { create } from "zustand";

import type { RehearsalStateEvent } from "@/lib/ipc";

/** UI-side state for a live Sim Con rehearsal: whether one is running, who the
 *  counterparty is, and the current phase (drives the speaking indicator). The
 *  phase is fed by `conva://rehearsal-state` events via the IPC bridge. */
export type RehearsalPhase = RehearsalStateEvent["phase"];

interface RehearsalState {
  active: boolean;
  personaTitle: string | null;
  phase: RehearsalPhase;
  /** Called when the user launches a rehearsal (before events arrive). */
  begin: (personaTitle: string) => void;
  /** Called when the user ends it locally. */
  end: () => void;
  applyPhase: (event: RehearsalStateEvent) => void;
}

export const useRehearsalStore = create<RehearsalState>((set) => ({
  active: false,
  personaTitle: null,
  phase: "thinking",

  begin: (personaTitle) => set({ active: true, personaTitle, phase: "thinking" }),
  end: () => set({ active: false, phase: "ended" }),
  applyPhase: (event) =>
    set(() =>
      event.phase === "ended"
        ? { active: false, phase: "ended" }
        : { phase: event.phase },
    ),
}));
