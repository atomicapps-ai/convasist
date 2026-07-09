import { AssistDock } from "@/components/AssistDock";
import { HealthStrip } from "@/components/HealthStrip";
import { StatusBar } from "@/components/StatusBar";
import { TranscriptView } from "@/components/transcript/TranscriptView";
import { useIpcBridge } from "@/lib/useIpcBridge";

export default function App() {
  useIpcBridge();

  return (
    <div className="flex h-full flex-col">
      <StatusBar />
      <TranscriptView />
      <AssistDock />
      <HealthStrip />
    </div>
  );
}
