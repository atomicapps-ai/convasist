import { useEffect, useState } from "react";

import { AssistDock } from "@/components/AssistDock";
import { ConsentGate } from "@/components/ConsentGate";
import { HealthStrip } from "@/components/HealthStrip";
import { SettingsPanel } from "@/components/SettingsPanel";
import { StatusBar } from "@/components/StatusBar";
import { TranscriptView } from "@/components/transcript/TranscriptView";
import { useIpcBridge } from "@/lib/useIpcBridge";
import { useAppStore } from "@/state/app";

export default function App() {
  useIpcBridge();
  const init = useAppStore((s) => s.init);
  const [showSettings, setShowSettings] = useState(false);

  useEffect(() => {
    void init();
  }, [init]);

  return (
    <div className="relative flex h-full flex-col">
      <StatusBar onToggleSettings={() => setShowSettings((v) => !v)} />
      {showSettings && <SettingsPanel onClose={() => setShowSettings(false)} />}
      <TranscriptView />
      <AssistDock />
      <HealthStrip />
      <ConsentGate />
    </div>
  );
}
