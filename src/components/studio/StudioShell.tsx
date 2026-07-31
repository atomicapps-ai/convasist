import { useEffect } from "react";

import { ConsentGate } from "@/components/ConsentGate";
import { PreparingOverlay } from "@/components/PreparingOverlay";
import { SaveConversationDialog } from "@/components/SaveConversationDialog";
import { CommandPalette } from "@/components/studio/CommandPalette";
import { NavRail } from "@/components/studio/NavRail";
import { StatusBar } from "@/components/studio/StatusBar";
import { TopBar } from "@/components/studio/TopBar";
import { ViewRouter } from "@/components/studio/ViewRouter";
import { Icon } from "@/components/ui/Icon";
import { UpdateBanner } from "@/components/UpdateBanner";
import { useAppStore } from "@/state/app";
import { useNavStore } from "@/state/nav";

/**
 * The DESKTOP shell (UI overhaul M2): a left NavRail selecting the active view,
 * a curved TopBar carrying the Core + Start/Stop control, and a routed content
 * area (ViewRouter). Web uses a separate WebShell over the same views. ⌘K opens
 * the command palette from anywhere.
 */
export function StudioShell() {
  const togglePalette = useNavStore((s) => s.togglePalette);
  const compact = useAppStore((s) => s.compact);
  const toggleCompact = useAppStore((s) => s.toggleCompact);

  // Global ⌘K / Ctrl+K → command palette.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        togglePalette();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [togglePalette]);

  return (
    <div className="flex h-full flex-col">
      <UpdateBanner />
      <div className="flex min-h-0 flex-1 gap-1 p-1">
        <NavRail />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <TopBar />
          <main className="min-h-0 flex-1 overflow-hidden">
            <ViewRouter />
          </main>
          <StatusBar />
        </div>
      </div>

      {/* Compact mode shrinks the window to a narrow strip; the header's
          Compact toggle can scroll out of reach, so guarantee a way back with
          an always-visible floating Expand control. */}
      {compact && (
        <button
          type="button"
          onClick={() => void toggleCompact()}
          title="Expand — leave compact mode"
          aria-label="Expand — leave compact mode"
          className="fixed right-2 top-2 z-50 flex items-center gap-1.5 rounded-full border border-border-strong bg-panel-raised px-3 py-1.5 text-[11px] font-semibold text-fg shadow-lg transition hover:brightness-110"
        >
          <Icon name="expand" size={14} />
          Expand
        </button>
      )}

      {/* Overlays — render above any view. */}
      <ConsentGate />
      <PreparingOverlay />
      <SaveConversationDialog />
      <CommandPalette />
    </div>
  );
}
