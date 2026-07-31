import { useEffect } from "react";

import { HealthStrip } from "@/components/HealthStrip";
import { ViewRouter } from "@/components/studio/ViewRouter";
import { GateView, useAccessGate } from "@/components/web/GateView";
import { WebSiteNav } from "@/components/web/WebSiteNav";
import { WebTopNav } from "@/components/web/WebTopNav";
import { useNavStore } from "@/state/nav";

/**
 * The WEB shell (web-only): a top navigation bar over a scrollable content
 * area. The desktop cockpit (StudioShell: left rail, meters, compact mode) is a
 * separate shell — both render the SAME view bodies via ViewRouter, so web and
 * desktop share content while each owns its chrome. This is where the web
 * experience is free to diverge without touching desktop.
 */
export function WebShell() {
  const togglePalette = useNavStore((s) => s.togglePalette);
  // Beta allowlist: signed in without access → the gate replaces the product.
  const gated = useAccessGate();

  // ⌘K / Ctrl+K → command palette (shared affordance).
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
    <div className="flex h-full flex-col bg-bg">
      {/* Band 1 — core WEBSITE links (owns brand + account, links out to the site). */}
      <WebSiteNav />
      {/* Band 2 — the app's own icon nav. */}
      <WebTopNav />
      {/* Band 3 — content. */}
      <main className="min-h-0 flex-1 overflow-y-auto">
        {gated ? <GateView /> : <ViewRouter />}
      </main>
      {/* Band 4 — app meters (mic/system + engine/latency). */}
      {!gated && <HealthStrip />}
    </div>
  );
}
