import { useEffect, useState } from "react";

import { AssistDock } from "@/components/AssistDock";
import { ConsentGate } from "@/components/ConsentGate";
import { HealthStrip } from "@/components/HealthStrip";
import { RagPanel } from "@/components/RagPanel";
import { SessionsPanel } from "@/components/SessionsPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { StatusBar } from "@/components/StatusBar";
import { TrackerRail } from "@/components/TrackerRail";
import { TranscriptView } from "@/components/transcript/TranscriptView";
import { useIpcBridge } from "@/lib/useIpcBridge";
import { useAppStore } from "@/state/app";

type Panel = "none" | "settings" | "library" | "sessions";

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
      />
      {panel === "settings" && (
        <SettingsPanel onClose={() => setPanel("none")} />
      )}
      {panel === "library" && <RagPanel onClose={() => setPanel("none")} />}
      {panel === "sessions" && (
        <SessionsPanel onClose={() => setPanel("none")} />
      )}
      <div className="flex min-h-0 flex-1">
        <TranscriptView />
        <TrackerRail />
      </div>
      <AssistDock />
      <HealthStrip />
      <ConsentGate />
    </div>
  );
}
