import { ConversationsPanel } from "@/components/ConversationsPanel";
import { DashboardView } from "@/components/dashboard/DashboardView";
import { FeaturesView } from "@/components/product/FeaturesView";
import { WhatsComingView } from "@/components/product/WhatsComingView";
import { WhatsNewView } from "@/components/product/WhatsNewView";
import { ProfileView } from "@/components/profile/ProfileView";
import { RagPanel } from "@/components/RagPanel";
import { SessionsPanel } from "@/components/SessionsPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { TranscriptView } from "@/components/transcript/TranscriptView";
import { useNavStore } from "@/state/nav";

/**
 * Renders the body of the active view. Shared by BOTH shells — the desktop
 * StudioShell (rail/cockpit) and the web WebShell (top nav) wrap the SAME view
 * bodies, so a new view is added here once and appears on both platforms. Only
 * the surrounding chrome differs per platform.
 */
export function ViewRouter() {
  const view = useNavStore((s) => s.view);
  const setView = useNavStore((s) => s.setView);
  const backToLive = () => setView("live");

  return (
    <>
      {view === "dashboard" && <DashboardView />}
      {view === "live" && <TranscriptView />}
      {view === "features" && <FeaturesView />}
      {view === "whatsnew" && <WhatsComingView />}
      {view === "releases" && <WhatsNewView />}
      {view === "settings" && <SettingsPanel onClose={backToLive} />}
      {view === "profile" && <ProfileView />}
      {view === "library" && <RagPanel onClose={backToLive} />}
      {view === "sessions" && <SessionsPanel onClose={backToLive} />}
      {view === "conversations" && <ConversationsPanel onClose={backToLive} />}
    </>
  );
}
