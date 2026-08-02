/**
 * The typed event contract for {@link ConvaBackend.subscribe}.
 *
 * Maps each logical event name to its payload type. The Tauri adapter binds
 * these to the `conva://*` events emitted by the Rust shell (see `EVENTS` in
 * `@/lib/ipc`); the web adapter binds the ones a browser can source (e.g.
 * hosted transcription → `transcriptSegment`, SSE Ally → `allyChunk`) and
 * no-ops the Layer-4-only ones.
 */

import type {
  AllyChunkEvent,
  AllySourcesEvent,
  AudioLevelEvent,
  AuthChangedEvent,
  ModelStatusEvent,
  RadarEvent,
  RehearsalStateEvent,
  SessionStateEvent,
  TrackerEvent,
  TranscriptSegment,
} from "@/lib/ipc";

export interface EventMap {
  transcriptSegment: TranscriptSegment;
  audioLevel: AudioLevelEvent;
  sessionState: SessionStateEvent;
  allyChunk: AllyChunkEvent;
  modelStatus: ModelStatusEvent;
  allySources: AllySourcesEvent;
  radar: RadarEvent;
  tracker: TrackerEvent;
  authChanged: AuthChangedEvent;
  rehearsalState: RehearsalStateEvent;
}

/** Handle returned by `subscribe`; call to stop receiving the event. */
export type Unsubscribe = () => void;

/**
 * Maps the {@link EventMap} keys to the shell's `conva://*` channel names.
 * Kept identical to `EVENTS` in `@/lib/ipc` so the Tauri adapter is a 1:1 bind.
 */
export const EVENT_CHANNEL: Record<keyof EventMap, string> = {
  transcriptSegment: "conva://transcript-segment",
  audioLevel: "conva://audio-level",
  sessionState: "conva://session-state",
  allyChunk: "conva://ally-chunk",
  modelStatus: "conva://model-status",
  allySources: "conva://ally-sources",
  radar: "conva://radar",
  tracker: "conva://tracker",
  authChanged: "conva://auth-changed",
  rehearsalState: "conva://rehearsal-state",
};
