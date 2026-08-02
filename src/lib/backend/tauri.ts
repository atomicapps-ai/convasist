/**
 * Desktop adapter — binds {@link ConvaBackend} to the Rust shell via the
 * existing typed command wrappers (`@/lib/commands`) and `conva://*` events.
 *
 * This is a thin, 1:1 delegation: it re-exposes today's shell surface behind the
 * PAL so components can migrate off `commands.ts` onto `getBackend()` without any
 * behavior change. No new behavior lives here.
 */

import { listen } from "@tauri-apps/api/event";

import * as cmd from "@/lib/commands";
import {
  DESKTOP_CAPABILITIES,
  type Capabilities,
} from "@/lib/backend/capabilities";
import type { ConvaBackend } from "@/lib/backend/ConvaBackend";
import { EVENT_CHANNEL, type EventMap, type Unsubscribe } from "@/lib/backend/events";

export class TauriBackend implements ConvaBackend {
  async capabilities(): Promise<Capabilities> {
    // Static desktop descriptor for now. TODO: refine dynamic fields from the
    // shell — gpuBackend from the whisper-backend probe, systemAudio per-OS,
    // overlay.incog from incog_status() once that command exists.
    return DESKTOP_CAPABILITIES;
  }

  async subscribe<K extends keyof EventMap>(
    event: K,
    handler: (payload: EventMap[K]) => void,
  ): Promise<Unsubscribe> {
    return listen<EventMap[K]>(EVENT_CHANNEL[event], (e) => handler(e.payload));
  }

  config = {
    get: cmd.getConfig,
    save: cmd.saveConfig,
    export: cmd.exportConfig,
    import: cmd.importConfig,
  };

  providers = {
    registry: cmd.getProviderRegistry,
    setKey: cmd.setApiKey,
    keyStatus: cmd.providerKeyStatus,
    test: cmd.testProvider,
    listModels: cmd.listProviderModels,
  };

  ally = {
    run: cmd.ally,
  };

  audio = {
    listDevices: cmd.listAudioDevices,
    listWhisperModels: cmd.listWhisperModels,
    setDeepgramKey: cmd.setDeepgramKey,
    deepgramKeyStatus: cmd.deepgramKeyStatus,
  };

  session = {
    start: cmd.startSession,
    stop: cmd.stopSession,
  };

  recording = {
    start: cmd.startRecording,
    stop: cmd.stopRecording,
    status: cmd.recordingStatus,
  };

  rag = {
    ingest: cmd.ragIngest,
    ingestText: cmd.ragIngestText,
    list: cmd.ragList,
    setEnabled: cmd.ragSetEnabled,
    delete: cmd.ragDelete,
    download: cmd.ragDownload,
    syncLibrary: cmd.ragSyncLibrary,
    analyzeTerms: cmd.analyzeTerms,
    documentText: cmd.ragDocumentText,
  };

  secrets = {
    status: cmd.secretsStatus,
    export: cmd.secretsExport,
    import: cmd.secretsImport,
  };

  auth = {
    start: cmd.authStart,
    cancel: cmd.authCancel,
    signinPassword: cmd.authSigninPassword,
    signupPassword: cmd.authSignupPassword,
    status: cmd.authStatus,
    signout: cmd.authSignout,
    openUrl: cmd.openUrl,
  };

  conversations = {
    save: cmd.conversationSave,
    list: cmd.conversationList,
    load: cmd.conversationLoad,
    delete: cmd.conversationDelete,
  };

  simcon = {
    save: cmd.simconSave,
    list: cmd.simconList,
    load: cmd.simconLoad,
    delete: cmd.simconDelete,
    storeDocs: cmd.simconStoreDocs,
    prepare: cmd.simconPrepare,
    loadProfile: cmd.simconLoadProfile,
    generateDossier: cmd.simconGenerateDossier,
    generatePersonas: cmd.simconGeneratePersonas,
    choosePersona: cmd.simconChoosePersona,
    startRehearsal: cmd.simconStartRehearsal,
    rehearsalYourTurn: cmd.simconRehearsalYourTurn,
    rehearsalSay: cmd.simconRehearsalSay,
    setResearchKey: cmd.setTavilyKey,
    researchKeyStatus: cmd.tavilyKeyStatus,
  };

  usage = {
    summary: cmd.usageSummary,
    reset: cmd.usageReset,
  };

  sessions = {
    list: cmd.sessionList,
    load: cmd.sessionLoad,
    exportTranscript: cmd.exportTranscript,
  };

  diagnostics = {
    saveDebugLog: cmd.saveDebugLog,
  };

  hud = {
    open: cmd.openHud,
    close: cmd.closeHud,
    toggle: cmd.toggleHud,
    isOpen: cmd.hudIsOpen,
  };
}
