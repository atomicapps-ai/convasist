/**
 * ConvaBackend — the Platform Abstraction Layer (PAL) for the shared React UI.
 *
 * The same `src/` runs on desktop (Tauri) and web (browser). Components talk to
 * THIS interface, never to `invoke`/`fetch` directly, and branch on
 * {@link Capabilities} — never on `isTauri`. Platform identity is resolved
 * exactly once at bootstrap into a concrete backend (see `detect.ts`).
 *
 *   Desktop → TauriBackend  (invoke + `conva://*` events → the Rust shell)
 *   Web     → WebBackend    (fetch/WS → api.conva.app/v1 + getUserMedia; hosted-only)
 *
 * Canonical model + rationale: `conva_core/docs/technical/CONVA_ARCHITECTURE.md`
 * (§5) and `CONVA_SDLC_RELEASE_STRATEGY.md` §1.2 (adopted as Phase 0 of the SDLC).
 *
 * Method groups mirror the 48-command shell surface (`src/lib/commands.ts` ↔
 * `crates/conva-core/src/ipc.rs`); keep them in lockstep — a change to the shell
 * contract updates this interface AND both adapters in the same PR.
 */

import type {
  AllyKind,
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
  ProviderId,
  ProviderInfo,
  ProviderKeyStatus,
  RagDocument,
  SecretsStatus,
  SessionSummary,
  TranscriptSegment,
  UsageSummary,
  WhisperModelInfo,
} from "@/lib/ipc";
import type { Capabilities } from "@/lib/backend/capabilities";
import type { EventMap, Unsubscribe } from "@/lib/backend/events";

export interface ConvaBackend {
  /** What this platform can do. Resolved once at bootstrap; drives all UI gating. */
  capabilities(): Promise<Capabilities>;

  /** Subscribe to a live event. Returns an unsubscribe handle. */
  subscribe<K extends keyof EventMap>(
    event: K,
    handler: (payload: EventMap[K]) => void,
  ): Promise<Unsubscribe>;

  /** Portable settings (`conva.config.json`). Layer 1 (synced) + local cache. */
  config: {
    get(): Promise<AppConfig>;
    save(config: AppConfig): Promise<void>;
    /** Desktop-only: write config to a file for git. Web → unsupported. */
    export(path: string): Promise<void>;
    /** Desktop-only: load a config file, apply live, persist. Web → unsupported. */
    import(path: string): Promise<AppConfig>;
  };

  /** LLM provider registry + BYO keys (Layer 4 keyring on desktop) + Ally. */
  providers: {
    registry(): Promise<ProviderInfo[]>;
    /** Desktop-only (OS keyring). Web uses hosted inference, no BYO keys. */
    setKey(provider: ProviderId, key: string): Promise<void>;
    keyStatus(): Promise<ProviderKeyStatus[]>;
    /** Measured first-token latency (ms). */
    test(provider: ProviderId, model: string): Promise<number>;
    listModels(provider: ProviderId): Promise<ModelInfo[]>;
  };

  /** Ally: grounded assist. Answer streams back as `allyChunk`/`allySources`. */
  ally: {
    run(
      requestId: string,
      kind: AllyKind,
      question: string | null,
      segments: TranscriptSegment[],
    ): Promise<void>;
  };

  /** Audio devices + ASR models. Layer 4 (local) on desktop. */
  audio: {
    /** Desktop: enumerate OS devices. Web: mic only (getUserMedia). */
    listDevices(): Promise<AudioDevice[]>;
    /** Desktop-only: local whisper checkpoints. Web → []. */
    listWhisperModels(): Promise<WhisperModelInfo[]>;
    setDeepgramKey(key: string): Promise<void>;
    deepgramKeyStatus(): Promise<boolean>;
  };

  /** Live capture session lifecycle. Emits `sessionState`/`transcriptSegment`. */
  session: {
    /** Resolves to the session id. */
    start(): Promise<string>;
    stop(): Promise<void>;
  };

  /** Stereo call recording (you = left, them = right). Desktop-only (Layer 4). */
  recording: {
    /** Resolves to the WAV path. */
    start(): Promise<string>;
    /** Resolves to the saved path, or null if none was active. */
    stop(): Promise<string | null>;
    status(): Promise<boolean>;
  };

  /** Document vault + retrieval. Local on desktop; cloud (pgvector) on web. */
  rag: {
    /** Desktop: ingest files by path. Web: use `ingestText`/uploads. */
    ingest(paths: string[]): Promise<IngestReport[]>;
    ingestText(name: string, text: string): Promise<IngestReport>;
    list(): Promise<RagDocument[]>;
    setEnabled(id: string, enabled: boolean): Promise<void>;
    delete(id: string): Promise<void>;
    /** Desktop-only: write a doc back to a path. Web → download via browser. */
    download(id: string, dest: string): Promise<void>;
    /** Desktop-only: copy library originals into the repo `library/` folder. */
    syncLibrary(): Promise<string>;
    /** RAG-relevant phrases in a message, for transcript highlighting. */
    analyzeTerms(text: string): Promise<string[]>;
    /** Reconstruct a document's text by id (e.g. to view the Ally dossier). */
    documentText(id: string): Promise<string | null>;
  };

  /** Portable encrypted secrets (`conva.secrets.enc`). Desktop-only (Layer 4). */
  secrets: {
    status(): Promise<SecretsStatus>;
    export(dest?: string): Promise<string>;
    import(src?: string, overwrite?: boolean): Promise<string>;
  };

  /**
   * Account sign-in (Layer 1/2). Desktop uses PKCE + the `conva://` deep-link
   * return through the shared `getconva.com/auth/callback` page (Layer 2); web
   * uses the server-side code exchange + session cookie. `authChanged` fires
   * when an out-of-band OAuth sign-in completes.
   */
  auth: {
    /** OAuth: opens the system browser (desktop) / redirects (web). */
    start(provider?: string): Promise<void>;
    cancel(): Promise<void>;
    signinPassword(email: string, password: string): Promise<AuthStatus>;
    signupPassword(email: string, password: string): Promise<AuthStatus>;
    status(): Promise<AuthStatus>;
    signout(): Promise<void>;
    /** Open an external URL (e.g. the shared password-reset page). */
    openUrl(url: string): Promise<void>;
  };

  /** Named conversations with append semantics. Local on desktop; cloud on web. */
  conversations: {
    save(
      id: string | null,
      title: string | null,
      segments: TranscriptSegment[],
      linkedDocs: string[],
    ): Promise<Conversation>;
    list(): Promise<ConversationSummary[]>;
    load(id: string): Promise<Conversation>;
    delete(id: string): Promise<void>;
  };

  /** SimCon — Simulated Conversation records. Local on desktop; cloud on web. */
  simcon: {
    /** Create or update; an empty `id` mints a new record. */
    save(session: SimConSession): Promise<SimConSession>;
    list(): Promise<SimConSummary[]>;
    load(id: string): Promise<SimConSession>;
    delete(id: string): Promise<void>;
    /** Copy documents into this Sim Con's folder; returns paths to ingest. */
    storeDocs(title: string, paths: string[]): Promise<string[]>;
    /** Build the reusable knowledge profile (docs + research) and mark ready. */
    prepare(id: string): Promise<SimConSession>;
    /** Load a knowledge profile (attached docs + researched sources) by id. */
    loadProfile(profileId: string): Promise<KnowledgeProfile>;
    /** Generate the Ally prep dossier (saved to the library). */
    generateDossier(id: string): Promise<SimConSession>;
    /** Generate 3 counterparty personas with the LLM. */
    generatePersonas(id: string): Promise<SimConSession>;
    /** Record the persona the user will rehearse against. */
    choosePersona(id: string, personaId: string): Promise<SimConSession>;
    /** Start a live rehearsal (mic → persona LLM → Aura TTS). Returns session id. */
    startRehearsal(id: string): Promise<string>;
    /** End the user's current rehearsal turn now (manual "your turn"). */
    rehearsalYourTurn(): Promise<void>;
    /** Inject a typed turn (e.g. an Ally-suggested answer) as the user's turn. */
    rehearsalSay(text: string): Promise<void>;
    /** Store (empty clears) the Tavily web-research key. */
    setResearchKey(key: string): Promise<void>;
    /** Whether a web-research key is configured. */
    researchKeyStatus(): Promise<boolean>;
  };

  /**
   * Usage metering — LLM tokens per provider + Tavily searches. Desktop keeps a
   * local BYO-key ledger (`usage.json`); the hosted future reports server-side
   * credit balances (roadmap F8b).
   */
  usage: {
    summary(): Promise<UsageSummary>;
    /** Clear all counters; returns the emptied snapshot. */
    reset(): Promise<UsageSummary>;
  };

  /** Auto-persisted session transcripts + export. */
  sessions: {
    list(): Promise<SessionSummary[]>;
    load(id: string): Promise<TranscriptSegment[]>;
    /** Desktop-only: write Markdown to a path. Web → browser download. */
    exportTranscript(path: string, segments: TranscriptSegment[]): Promise<void>;
  };

  /** Diagnostics. */
  diagnostics: {
    /** Desktop-only: write a report to a log file (resolves to the path). */
    saveDebugLog(contents: string): Promise<string>;
  };

  /** Floating HUD panel (`src-tauri/src/hud.rs`). Desktop-only (Layer 4). */
  hud: {
    open(): Promise<void>;
    close(): Promise<void>;
    /** Resolves to the new state (true = open). */
    toggle(): Promise<boolean>;
    isOpen(): Promise<boolean>;
  };
}
