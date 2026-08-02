/**
 * Web adapter — SKELETON. Implements {@link ConvaBackend} for "conva Lite" in a
 * browser tab. Layers 1–3 route to `api.conva.app/v1` + the hosted-inference
 * proxy + `getUserMedia`; Layer 4 (loopback, local ASR, keyring, HUD, recording,
 * secrets, file-path I/O) is unsupported and the UI renders the honest degraded
 * state via `capabilities()`.
 *
 * This is the SPEC, not the implementation: methods are stubbed to make the
 * contract explicit and typecheck. `unsupported()` = Layer-4, never coming to
 * web; `todo()` = Layer 1–3, to be wired to the named endpoint (roadmap 1.3/1.4).
 * Nothing here should silently pretend to work.
 */

import {
  WEB_CAPABILITIES,
  type Capabilities,
} from "@/lib/backend/capabilities";
import type { ConvaBackend } from "@/lib/backend/ConvaBackend";
import type { EventMap, Unsubscribe } from "@/lib/backend/events";
import * as webAuth from "@/lib/backend/webAuth";
import type {
  AppConfig,
  AudioDevice,
  AuthStatus,
  Conversation,
  ConversationSummary,
  KnowledgeProfile,
  SimConSession,
  SimConSummary,
  IngestReport,
  ModelInfo,
  ProviderInfo,
  ProviderKeyStatus,
  RagDocument,
  SecretsStatus,
  SessionSummary,
  TranscriptSegment,
  UsageSummary,
  WhisperModelInfo,
} from "@/lib/ipc";

/** Capability genuinely absent in a browser (Layer 4) — never coming to web. */
export class UnsupportedOnWebError extends Error {
  constructor(feature: string) {
    super(`"${feature}" is a desktop-only capability — not available on the web.`);
    this.name = "UnsupportedOnWebError";
  }
}

function unsupported<T>(feature: string): Promise<T> {
  return Promise.reject(new UnsupportedOnWebError(feature));
}

/** Layer 1–3 method not yet wired to the API. Roadmap 1.3 (proxy) / 1.4 (adapter). */
function todo<T>(endpoint: string): Promise<T> {
  return Promise.reject(
    new Error(`WebBackend not implemented yet — will call ${endpoint}`),
  );
}

export class WebBackend implements ConvaBackend {
  async capabilities(): Promise<Capabilities> {
    return WEB_CAPABILITIES;
  }

  async subscribe<K extends keyof EventMap>(
    event: K,
    handler: (payload: EventMap[K]) => void,
  ): Promise<Unsubscribe> {
    // `authChanged` is live: sign-in/out in this tab, or the login page
    // completing in another same-origin tab (storage event), both fire it.
    if (event === "authChanged") {
      return webAuth.onAuthChanged((status) => {
        handler({ status, error: null } as EventMap[K]);
      });
    }
    // TODO(1.4): bind browser-sourced events — hosted transcription →
    // `transcriptSegment`, SSE Ally → `allyChunk`/`allySources`, session state
    // from the mic pipeline. Layer-4-only events (`audioLevel` beyond the mic,
    // desktop `sessionState`) stay no-ops.
    if (import.meta.env?.DEV) console.warn(`[web] subscribe("${event}") is a no-op (not wired yet)`);
    return () => {};
  }

  config = {
    get: (): Promise<AppConfig> => todo("GET /v1/settings"),
    save: (_config: AppConfig): Promise<void> => todo("PUT /v1/settings"),
    export: (_path: string): Promise<void> => unsupported("config.export"),
    import: (_path: string): Promise<AppConfig> => unsupported("config.import"),
  };

  providers = {
    registry: (): Promise<ProviderInfo[]> => todo("GET /v1/models"),
    setKey: (): Promise<void> => unsupported("providers.setKey (BYO keys)"),
    keyStatus: (): Promise<ProviderKeyStatus[]> => Promise.resolve([]),
    test: (): Promise<number> => unsupported("providers.test (BYO keys)"),
    listModels: (): Promise<ModelInfo[]> => todo("GET /v1/models"),
  };

  ally = {
    run: (): Promise<void> => todo("POST /v1/inference/complete (SSE)"),
  };

  audio = {
    listDevices: (): Promise<AudioDevice[]> =>
      todo("navigator.mediaDevices.enumerateDevices"),
    listWhisperModels: (): Promise<WhisperModelInfo[]> => Promise.resolve([]),
    setDeepgramKey: (): Promise<void> => unsupported("audio.setDeepgramKey"),
    deepgramKeyStatus: (): Promise<boolean> => Promise.resolve(false),
  };

  session = {
    start: (): Promise<string> => todo("getUserMedia → hosted transcription"),
    stop: (): Promise<void> => todo("stop the mic pipeline"),
  };

  recording = {
    start: (): Promise<string> => unsupported("recording.start"),
    stop: (): Promise<string | null> => unsupported("recording.stop"),
    status: () => Promise.resolve(false),
  };

  rag = {
    ingest: (): Promise<IngestReport[]> => unsupported("rag.ingest (file paths)"),
    ingestText: (_name: string, _text: string): Promise<IngestReport> =>
      todo("POST /v1/library (server-side embeddings)"),
    list: (): Promise<RagDocument[]> => todo("GET /v1/library"),
    setEnabled: (): Promise<void> => todo("PATCH /v1/library/:id"),
    delete: (): Promise<void> => todo("DELETE /v1/library/:id"),
    download: (): Promise<void> => unsupported("rag.download (file path)"),
    syncLibrary: (): Promise<string> => unsupported("rag.syncLibrary (git)"),
    analyzeTerms: (): Promise<string[]> => Promise.resolve([]),
    documentText: (): Promise<string | null> => todo("GET /v1/library/:id/text"),
  };

  secrets = {
    status: (): Promise<SecretsStatus> => unsupported("secrets.status"),
    export: (): Promise<string> => unsupported("secrets.export"),
    import: (): Promise<string> => unsupported("secrets.import"),
  };

  auth = {
    // Full/OAuth sign-in hands off to the shared getconva.com login page
    // (Layer 2) and returns; the session lands in the same-origin
    // `conva.session` record this adapter reads. See webAuth.ts.
    start: (provider?: string): Promise<void> => {
      webAuth.loginRedirect(provider);
      return Promise.resolve();
    },
    cancel: (): Promise<void> => Promise.resolve(),
    signinPassword: (e: string, p: string): Promise<AuthStatus> =>
      webAuth.signinPassword(e, p),
    signupPassword: (e: string, p: string): Promise<AuthStatus> =>
      webAuth.signupPassword(e, p),
    status: (): Promise<AuthStatus> => Promise.resolve(webAuth.status()),
    signout: (): Promise<void> => webAuth.signout(),
    openUrl: (url: string): Promise<void> => {
      window.open(url, "_blank", "noopener");
      return Promise.resolve();
    },
  };

  conversations = {
    save: (): Promise<Conversation> => todo("POST /v1/conversations"),
    list: (): Promise<ConversationSummary[]> => todo("GET /v1/conversations"),
    load: (): Promise<Conversation> => todo("GET /v1/conversations/:id"),
    delete: (): Promise<void> => todo("DELETE /v1/conversations/:id"),
  };

  simcon = {
    save: (): Promise<SimConSession> => todo("POST /v1/simcon"),
    list: (): Promise<SimConSummary[]> => todo("GET /v1/simcon"),
    load: (): Promise<SimConSession> => todo("GET /v1/simcon/:id"),
    delete: (): Promise<void> => todo("DELETE /v1/simcon/:id"),
    storeDocs: (): Promise<string[]> =>
      unsupported("simcon.storeDocs (local file paths)"),
    prepare: (): Promise<SimConSession> => todo("POST /v1/simcon/:id/prepare"),
    loadProfile: (): Promise<KnowledgeProfile> =>
      todo("GET /v1/simcon/profiles/:id"),
    generateDossier: (): Promise<SimConSession> =>
      todo("POST /v1/simcon/:id/dossier"),
    generatePersonas: (): Promise<SimConSession> =>
      todo("POST /v1/simcon/:id/personas"),
    choosePersona: (): Promise<SimConSession> =>
      todo("PATCH /v1/simcon/:id/persona"),
    startRehearsal: (): Promise<string> =>
      unsupported("simcon.startRehearsal (desktop audio)"),
    rehearsalYourTurn: (): Promise<void> =>
      unsupported("simcon.rehearsalYourTurn (desktop audio)"),
    rehearsalSay: (): Promise<void> =>
      unsupported("simcon.rehearsalSay (desktop audio)"),
    setResearchKey: (): Promise<void> =>
      unsupported("simcon.setResearchKey (server-side on web)"),
    researchKeyStatus: () => Promise.resolve(false),
  };

  usage = {
    summary: (): Promise<UsageSummary> => todo("GET /v1/usage"),
    reset: (): Promise<UsageSummary> => todo("POST /v1/usage/reset"),
  };

  sessions = {
    list: (): Promise<SessionSummary[]> => todo("GET /v1/sessions"),
    load: (): Promise<TranscriptSegment[]> => todo("GET /v1/sessions/:id"),
    exportTranscript: (): Promise<void> => unsupported("sessions.exportTranscript (file path)"),
  };

  diagnostics = {
    saveDebugLog: (): Promise<string> => unsupported("diagnostics.saveDebugLog (file)"),
  };

  hud = {
    open: (): Promise<void> => unsupported("hud.open"),
    close: (): Promise<void> => unsupported("hud.close"),
    toggle: (): Promise<boolean> => unsupported("hud.toggle"),
    isOpen: () => Promise.resolve(false),
  };
}
