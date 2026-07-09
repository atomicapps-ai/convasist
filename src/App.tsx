import { useEffect, useState } from "react";

import { AssistDock } from "@/components/AssistDock";
import { ConsentGate } from "@/components/ConsentGate";
import { HealthStrip } from "@/components/HealthStrip";
import { RagPanel } from "@/components/RagPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { StatusBar } from "@/components/StatusBar";
import { TranscriptView } from "@/components/transcript/TranscriptView";
import { useIpcBridge } from "@/lib/useIpcBridge";
import { useAppStore } from "@/state/app";

type Panel = "none" | "settings" | "library";

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
      />
      {panel === "settings" && (
        <SettingsPanel onClose={() => setPanel("none")} />
      )}
      {panel === "library" && <RagPanel onClose={() => setPanel("none")} />}
      <TranscriptView />
      <AssistDock />
      <HealthStrip />
      <ConsentGate />
    </div>
  );
}
