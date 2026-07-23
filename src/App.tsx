import { useEffect, useState } from "react";

import { AssistDock } from "@/components/AssistDock";
import { ConsentGate } from "@/components/ConsentGate";
import { ConversationsPanel } from "@/components/ConversationsPanel";
import { HealthStrip } from "@/components/HealthStrip";
import { PreparingOverlay } from "@/components/PreparingOverlay";
import { RagPanel } from "@/components/RagPanel";
import { SaveConversationDialog } from "@/components/SaveConversationDialog";
import { SessionsPanel } from "@/components/SessionsPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { StatusBar } from "@/components/StatusBar";
import { TrackerRail } from "@/components/TrackerRail";
import { TranscriptView } from "@/components/transcript/TranscriptView";
import { useIpcBridge } from "@/lib/useIpcBridge";
import { useAppStore } from "@/state/app";

type Panel = "none" | "settings" | "library" | "sessions" | "conversations";

export default function App() {
  useIpcBridge();
  const init = useAppStore((s) => s.init);
  const [panel, setPanel] = useState<Panel>("none");
  const toggle = (which: Panel) =>
    setPanel((current) => (current === which ? "none" : which));

  useEffect(() => {
    void init();
  }, [init]);

  return (
    <div className="relative flex h-full flex-col">
      <StatusBar
        onToggleSettings={() => toggle("settings")}
        onToggleLibrary={() => toggle("library")}
        onToggleSessions={() => toggle("sessions")}
        onToggleConversations={() => toggle("conversations")}
      />
      {panel === "settings" && (
        <SettingsPanel onClose={() => setPanel("none")} />
      )}
      {panel === "library" && <RagPanel onClose={() => setPanel("none")} />}
      {panel === "sessions" && (
        <SessionsPanel onClose={() => setPanel("none")} />
      )}
      {panel === "conversations" && (
        <ConversationsPanel onClose={() => setPanel("none")} />
      )}
      <div className="flex min-h-0 flex-1">
        <TranscriptView />
        <TrackerRail />
      </div>
      <AssistDock />
      <HealthStrip />
      <ConsentGate />
      <PreparingOverlay />
      <SaveConversationDialog />
    </div>
  );
}
