import { useTranscriptStore } from "@/state/transcript";

/**
 * Full-screen loading state while the session is starting: model load,
 * engine connect, and — on the first GPU run — minutes of shader
 * compilation. Driven by the backend's `preparing` session events so the
 * user never stares at a dead UI wondering if the app hung.
 */
export function PreparingOverlay() {
  const session = useTranscriptStore((s) => s.session);
  if (session.state !== "preparing") return null;

  return (
    <div
      className="absolute inset-0 z-40 flex flex-col items-center justify-center gap-4 bg-bg/85 backdrop-blur-sm"
      role="status"
      aria-live="polite"
    >
      <span
        className="h-10 w-10 animate-spin rounded-full border-2 border-ai/30 border-t-ai"
        aria-hidden
      />
      <p className="max-w-sm px-6 text-center text-sm text-fg">
        {session.message}
      </p>
      <p className="max-w-sm px-6 text-center text-xs text-fg-faint">
        First runs can take a while (model downloads, GPU shader compilation).
        Later starts are fast.
      </p>
    </div>
  );
}
