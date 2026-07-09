import { useAppStore } from "@/state/app";

/**
 * Recording-consent acknowledgment (design §7.1). Blocks the first capture
 * session until acknowledged; the shell also refuses server-side.
 */
export function ConsentGate() {
  const config = useAppStore((s) => s.config);
  const acknowledge = useAppStore((s) => s.acknowledgeConsent);

  if (!config || config.consent_acknowledged) return null;

  return (
    <div
      className="absolute inset-0 z-50 flex items-center justify-center bg-bg/80 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-labelledby="consent-title"
    >
      <div className="mx-4 max-w-md rounded-lg border border-border bg-panel p-6">
        <h2 id="consent-title" className="text-sm font-semibold">
          Before you start listening
        </h2>
        <p className="mt-3 text-sm leading-relaxed text-fg-muted">
          convasist transcribes both sides of conversations on this computer.
          Many jurisdictions (including California) require{" "}
          <strong className="text-fg">consent from every participant</strong>{" "}
          before recording or transcribing a conversation. You are responsible
          for obtaining it. A red REC indicator is always visible while a
          session is live.
        </p>
        <button
          type="button"
          onClick={() => void acknowledge()}
          className="mt-5 w-full rounded-md bg-ai/90 px-4 py-2 text-sm font-semibold text-bg hover:bg-ai"
        >
          I understand — I&apos;ll get consent
        </button>
      </div>
    </div>
  );
}
