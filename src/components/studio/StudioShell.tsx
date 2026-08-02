import { useEffect } from "react";

import { ConsentGate } from "@/components/ConsentGate";
import { ConversationsPanel } from "@/components/ConversationsPanel";
import { DashboardView } from "@/components/dashboard/DashboardView";
import { FeaturesView } from "@/components/product/FeaturesView";
import { WhatsComingView } from "@/components/product/WhatsComingView";
import { PreparingOverlay } from "@/components/PreparingOverlay";
import { ProfileView } from "@/components/profile/ProfileView";
import { RehearsalBar } from "@/components/simcon/RehearsalBar";
import { SimConView } from "@/components/simcon/SimConView";
import { GateView, useAccessGate } from "@/components/web/GateView";
import { RagPanel } from "@/components/RagPanel";
import { SaveConversationDialog } from "@/components/SaveConversationDialog";
import { SessionsPanel } from "@/components/SessionsPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { CommandPalette } from "@/components/studio/CommandPalette";
import { NavRail } from "@/components/studio/NavRail";
import { StatusBar } from "@/components/studio/StatusBar";
import { TopBar } from "@/components/studio/TopBar";
import { TranscriptView } from "@/components/transcript/TranscriptView";
import { Icon } from "@/components/ui/Icon";
import { UpdateBanner } from "@/components/UpdateBanner";
import { useAppStore } from "@/state/app";
import { useNavStore } from "@/state/nav";

/** The live cockpit is the whole three-column instrument (transcript · spine ·
 *  Ally). Meters live in the top bar; Ally actions live in the Ally column. */
function LiveView() {
  return <TranscriptView />;
}

/**
 * The Studio shell (UI overhaul M2). One instrument: a left NavRail selecting
 * the active view, a curved TopBar carrying the Core + Start/Stop control, and
 * a routed content area. The former dropdown panels (Settings/Library/Sessions/
 * Conversations) are now first-class views; ⌘K opens the command palette from
 * anywhere. Replaces the old StatusBar + inline-panel App layout.
 */
export function StudioShell() {
  const view = useNavStore((s) => s.view);
  const setView = useNavStore((s) => s.setView);
  const togglePalette = useNavStore((s) => s.togglePalette);
  const compact = useAppStore((s) => s.compact);
  const toggleCompact = useAppStore((s) => s.toggleCompact);
  const backToLive = () => setView("live");
  // Beta allowlist (web): signed in without access → the gate replaces the
  // product surface, whatever view the rail selects.
  const gated = useAccessGate();

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
            {gated ? (
              <GateView />
            ) : (
              <>
                {view === "dashboard" && <DashboardView />}
                {view === "live" && <LiveView />}
                {view === "features" && <FeaturesView />}
                {view === "whatsnew" && <WhatsComingView />}
                {view === "settings" && <SettingsPanel onClose={backToLive} />}
                {view === "profile" && <ProfileView />}
                {view === "library" && <RagPanel onClose={backToLive} />}
                {view === "sessions" && <SessionsPanel onClose={backToLive} />}
                {view === "conversations" && (
                  <ConversationsPanel onClose={backToLive} />
                )}
                {view === "simcon" && <SimConView />}
              </>
            )}
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
      <RehearsalBar />
      <ConsentGate />
      <PreparingOverlay />
      <SaveConversationDialog />
      <CommandPalette />
    </div>
  );
}
