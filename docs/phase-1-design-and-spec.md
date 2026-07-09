# convasist — Phase 1 Design & Specification

**Status:** DRAFT — awaiting owner review. No application code is written until the tech stack (§2) and feature list (§4) are explicitly approved.
**Scope:** Phase 1 — audio interception, real-time dual-channel transcription UI, inline AI processing, local RAG engine.
**Date:** 2026-07-09

---

## 1. Executive summary

convasist is a real-time AI conversation assistant that listens to both sides of any conversation happening on the host computer (mic = what you say, system audio = what you hear), transcribes both streams live into a dual-column chat UI, and lets an AI agent — grounded by a local RAG corpus of your reference documents — jump in on any part of the conversation at any moment.

**The single decision that shapes everything else: this cannot be a browser app.** Browsers have no API to capture system output audio (the "what you hear" stream). That capability alone forces a native desktop architecture. Full contrast in §2.2.

**Recommended stack (summary):**

| Layer | Recommendation | Why |
|---|---|---|
| Shell / packaging | **Tauri 2** (Rust core + system WebView) | Native Rust audio/ML in-process, ~10 MB installer, first-class IPC streaming |
| UI | **React 19 + TypeScript + Tailwind** | Matches team's existing stack; WebView renders it at 60fps easily |
| Audio capture | **WASAPI loopback + capture** (Windows), CoreAudio/ScreenCaptureKit (macOS later) | The only OS-level, no-driver way to intercept both streams |
| Transcription | **Pluggable engine trait** — local `whisper.cpp` (GPU) default, Deepgram streaming as opt-in cloud mode | Local = private + free; cloud = lowest latency; the trait makes it a config choice, not an architecture choice |
| VAD | Silero VAD via ONNX Runtime | ~1 ms per frame; gates ASR so silence costs nothing |
| RAG embeddings | `fastembed` (BGE-small-en-v1.5, 384-dim, local CPU) | <5 ms query embedding, no API key, no network |
| Vector store | **LanceDB** (embedded, in-process) | Rust-native, hybrid vector+FTS search, zero server to run |
| LLM | **Claude API, streaming** (`claude-sonnet-5` default; `claude-haiku-4-5` for cheap proactive suggestions) | Best quality per token; streaming keeps perceived latency <500 ms to first word |

**Realistic latency budget (mic → text on screen):** ~300–700 ms for partial transcripts, 1–2 s for finalized text. Breakdown and tuning levers in §2.5. Anyone promising "near-zero" end-to-end speech-to-text is ignoring physics — the design instead makes every stage measurable and puts a hard number on each.

---

## 2. Tech stack — analysis and recommendation

### 2.1 The gating constraint: OS-level audio interception

Phase 1 requires capturing **two** streams:

1. **Mic (Audio In)** — what the user says. Every platform, including browsers, can do this.
2. **System output / loopback (Audio Out)** — what the user *hears* (the other party on a Zoom/Meet/Teams/phone-bridge call, a video, any app). This is the differentiating stream, and it is the constraint that eliminates the browser:

| Capability | Browser (Web Audio / WebRTC / WASM) | Native desktop |
|---|---|---|
| Mic capture | ✅ `getUserMedia` | ✅ |
| **System output capture** | ❌ No API. `getDisplayMedia({audio:true})` captures *tab* audio only (Chrome), not other apps, and demands a screen-share permission prompt every session | ✅ WASAPI loopback (Windows) / CoreAudio taps & ScreenCaptureKit (macOS) / PipeWire monitors (Linux) — silent, driverless, all-app |
| Per-app audio isolation | ❌ | ✅ Windows process-loopback (Win10 2004+) can capture *only* Zoom's output, excluding music/notifications |
| Capture while minimized / global hotkeys | ❌ (tab throttling, no global hotkeys) | ✅ |
| Local ASR inference | ⚠️ whisper.cpp-WASM exists but runs 3–10× slower than native; no CUDA/Metal | ✅ Full GPU acceleration |
| Local vector DB + embeddings | ⚠️ IndexedDB + WASM embeddings — slow, memory-capped | ✅ mmap'd ANN index, native SIMD |
| Audio thread scheduling | ⚠️ 128-sample AudioWorklet quantum is fine, but everything after it fights the event loop | ✅ Dedicated real-time threads, lock-free ring buffers |

**Verdict: the browser fails the mission's core requirement (bidirectional interception) outright — this is a hard blocker, not a performance tradeoff.** Per the mission's own escape hatch, we go native.

### 2.2 Native desktop options — contrast

| Criterion | **Tauri 2** (Rust core + WebView UI) | Electron + native Node modules (napi-rs) | Rust/Python + FFI (e.g., PySide) | C++ / .NET (WinUI/WPF) |
|---|---|---|---|---|
| Audio + ASR + RAG runtime | Rust, in-process, real-time threads | Rust via N-API, but marshals through Node's event loop | Python orchestrates; GIL contention risks audio thread starvation | Best-in-class |
| UI development speed | React/TS — team's home turf | React/TS — team's home turf | Qt widgets — slow iteration | Slowest; XAML |
| Installer size / RAM | ~10–15 MB / ~80–150 MB | ~90–150 MB / ~300–500 MB | ~150 MB+ (bundled interpreter) | Small / small |
| IPC for streaming transcripts | Built-in channels + events; binary-capable | WebContents IPC; fine | Custom (websocket/zmq) — extra moving part | N/A (single process) |
| macOS path later | ✅ same codebase | ✅ same codebase | ✅ painful packaging | ❌ (C++) / ❌ (.NET WinUI) |
| ML ecosystem access | whisper-rs, ort (ONNX), fastembed, lancedb — all native Rust | Same crates via napi bridge | Richest (PyTorch) — but Phase 1 needs inference only, which ONNX/GGML cover | ONNX Runtime C API |
| Risk | WebView quirks (WebView2 on Win = Chromium, low risk) | Chromium memory/battery tax on an always-on listener | GIL + packaging | Single-platform lock-in, slowest team velocity |

**Recommendation: Tauri 2.** It is the only option that gives us (a) Rust real-time audio and inference in the same process as the pipeline — no IPC hop between capture and ASR, (b) the team's existing React/TypeScript muscle for the UI, and (c) a footprint appropriate for an app that runs all day next to a video call. An always-on assistant competing with Zoom + Chrome for RAM should not itself be a second Chromium.

**Runner-up:** Electron + a Rust napi module is acceptable if we later hit a WebView limitation; the Rust core (§3) is deliberately shell-agnostic so this swap would cost days, not weeks.

**Explicitly rejected:** browser/WASM (fails §2.1), Python core (GIL vs. real-time audio), pure C++/.NET (single-platform, slowest iteration, no team leverage).

### 2.3 Recommended stack — full detail

**Core (Rust, `src-tauri/`):**

| Concern | Choice | Notes |
|---|---|---|
| Mic capture | `cpal` | Cross-platform capture abstraction, callback-driven |
| Loopback capture (Win) | `wasapi` crate (or `windows-rs` direct) | Event-driven shared-mode loopback on the default render endpoint; 10 ms period. Optional per-process loopback for "capture only Zoom" |
| Loopback (macOS, Phase 1.5) | ScreenCaptureKit audio / CoreAudio process taps (14.4+) | Same `AudioSource` trait; BlackHole virtual device as documented fallback |
| Resampling | `rubato` | Everything normalized to 16 kHz mono f32 before ASR |
| Ring buffers | `rtrb` (lock-free SPSC) | Zero allocation, zero locks on the audio callback thread — non-negotiable |
| VAD | Silero VAD v5 via `ort` (ONNX Runtime) | ~1 ms/32 ms frame on CPU; gates chunking + gives utterance boundaries |
| ASR (local default) | `whisper-rs` (whisper.cpp) — `base.en`/`small.en` or distil-whisper, CUDA/Vulkan/Metal | VAD-gated streaming with rolling context; greedy decode for partials, beam for finals |
| ASR (cloud opt-in) | Deepgram streaming WebSocket | ~150–300 ms partials; behind the same `TranscriptionEngine` trait — config toggle, not a fork |
| Embeddings | `fastembed` (BGE-small-en-v1.5) | Local, CPU, 384-dim; ~4 ms/query, ~1k chunks/min ingest |
| Vector store | `lancedb` (embedded) | ANN + full-text hybrid, versioned, single data dir; no server process |
| Document parsing | `pdfium-render` (PDF), `docx-rs`, plain md/txt/html | Runs on a background ingestion worker, never the audio path |
| LLM client | `anthropic` SDK / raw `reqwest` SSE streaming | Streaming always; `claude-sonnet-5` for on-demand assists, `claude-haiku-4-5` for high-frequency proactive suggestions |
| Async runtime | `tokio` | I/O only — audio DSP stays on dedicated OS threads, never on the async executor |

**UI (TypeScript, `src/`):** React 19, Tailwind CSS 4, shadcn/ui primitives, Zustand for transcript state, `@tauri-apps/api` events/channels for the stream. Virtualized transcript list (`virtua`) so a 3-hour conversation scrolls at 60fps.

**Proposed repository layout** (per the mandated structure — `docs/` + app codebase root):

```
convasist/
├── docs/                        # blueprints, specs, phase docs (this file)
├── src-tauri/                   # Rust core
│   ├── src/
│   │   ├── audio/               # capture (mic/loopback), resample, ring buffers, AEC
│   │   ├── asr/                 # TranscriptionEngine trait, whisper impl, deepgram impl, VAD
│   │   ├── rag/                 # ingest, chunking, embeddings, lancedb store, retriever
│   │   ├── ai/                  # LLM client, context builder, assist orchestration
│   │   ├── session/             # conversation state, persistence (SQLite), export
│   │   └── ipc/                 # typed events/commands to the UI
│   └── tauri.conf.json
├── src/                         # React UI
│   ├── components/{transcript,assist,rag,settings}/
│   ├── state/                   # Zustand stores fed by IPC events
│   └── styles/
└── models/                      # gitignored; whisper/VAD/embedding models fetched on first run
```

### 2.4 Threading & data-flow model (the performance contract)

```
[Mic]───cpal cb──▶(SPSC ring)──▶┐
                                ├─▶ [Audio worker thread]: resample→AEC?→VAD→utterance chunks
[Loopback]─wasapi cb─▶(SPSC)──▶┘                │
                                                ▼ (bounded channel, per stream)
                                  [ASR worker(s)]: partial + final segments
                                                │
                                                ▼ tokio broadcast
                     ┌──────────────────────────┼─────────────────────────┐
                     ▼                          ▼                         ▼
              [IPC → UI events]        [Session store (SQLite)]   [AI orchestrator]
                                                                  │ (context = last N turns
                                                                  ▼  + RAG top-k)
                                                            [Claude stream] ──▶ IPC → UI
```

Rules the code must obey (these become CI-enforceable conventions):

1. **Audio callbacks do nothing but copy into a ring buffer.** No allocation, no locks, no logging, no syscalls.
2. **Every cross-stage handoff is a bounded queue with an explicit overflow policy** (drop-oldest for partials, never-drop for finals). Backpressure is designed, not discovered.
3. **Every stage stamps a monotonic capture-timestamp**; the UI can render per-stage latency live (see §6, HUD). What we don't measure, we can't keep fast.
4. **ASR, embedding, and LLM work never share a thread pool with audio.**

### 2.5 Latency budget (Windows, local whisper `base.en` on GPU)

| Stage | Budget (P50) | Notes / levers |
|---|---|---|
| Capture buffer (WASAPI shared, event-driven) | 10 ms | 10 ms period; exclusive mode ~3 ms if ever needed |
| Resample + VAD frame | ~2 ms | SIMD resampler, Silero on CPU |
| Chunk assembly (streaming window) | 100–300 ms | Main tunable: how much audio ASR sees before emitting a partial |
| ASR partial decode | 150–350 ms | GPU whisper `base.en`; drops to ~150–250 ms with Deepgram cloud incl. network |
| IPC + React render | <16 ms | One frame; events batched per 50 ms tick |
| **Partial text on screen** | **~0.3–0.7 s** | Finalized (beam, punctuated) text: 1–2 s behind live audio |
| RAG retrieve (embed query + ANN top-10) | <15 ms | Local, in-process |
| LLM first streamed token | 300–600 ms | Claude streaming; suggestion cards render word-by-word |

---

## 3. Architecture overview — module boundaries

Five modules, each behind a trait/interface so implementations swap without touching neighbors:

| Module | Owns | Public surface |
|---|---|---|
| **Audio Layer** | Device enumeration, mic + loopback capture, resample, VAD, (later) echo cancellation | `AudioSource` trait → stream of timestamped 16 kHz mono frames tagged `Inbound` (heard) / `Outbound` (spoken) |
| **Transcription Layer** | Utterance chunking, ASR engines, partial/final lifecycle | `TranscriptionEngine` trait → `TranscriptEvent { stream, seq, text, is_final, t_start, t_end, latency }` |
| **UI Layer** | Dual-column transcript, assist surfaces, RAG library, settings, hotkeys | Consumes typed IPC events; issues typed commands (`start/stop`, `assist(scope)`, `ingest(paths)`) |
| **RAG / Vector Layer** | Ingestion pipeline, chunking, embeddings, LanceDB store, hybrid retriever | `retrieve(query, k) → Vec<ScoredChunk>`; `ingest(doc) → IngestReport` |
| **AI Orchestration Layer** | Context assembly (transcript window + RAG hits + pinned facts), prompt templates, Claude streaming, trigger policy (manual/proactive) | `assist(request) → token stream`; emits `SuggestionEvent`s |

Everything persists locally: transcripts + sessions in SQLite, vectors in LanceDB, documents in an app-data folder. **No audio or transcript leaves the machine unless cloud ASR or the LLM call is invoked** — and the LLM call sends text context only, never audio.

---

## 4. Phase 1 feature breakdown

Legend: **[C]** core — Phase 1 ships with it; **[S]** stretch — in scope if schedule allows; **[D]** deferred — designed-for, built later.

### 4.1 Audio Layer

| # | Feature | Tier |
|---|---|---|
| A1 | Mic capture (default input device), event-driven, 16 kHz normalized | C |
| A2 | System loopback capture (default output device) — WASAPI loopback | C |
| A3 | Device picker + hot-swap on device change/unplug (auto-recover, never crash the session) | C |
| A4 | Live VU meters per stream + silence/health watchdog ("mic went dead" warning) | C |
| A5 | VAD gating (no ASR cost during silence; utterance boundary detection) | C |
| A6 | Per-process loopback ("listen to Zoom only") | S |
| A7 | Echo cancellation (AEC3) for open-speaker setups — Phase 1 documents "use a headset"; AEC removes that caveat | S |
| A8 | Raw audio recording to disk (opt-in), synced to transcript timestamps | S |
| A9 | macOS capture backends (ScreenCaptureKit / CoreAudio taps) | D |

### 4.2 Transcription Layer

| # | Feature | Tier |
|---|---|---|
| T1 | Streaming local ASR (whisper.cpp GPU, `base.en` default, model picker) with partial → final lifecycle | C |
| T2 | `TranscriptionEngine` trait + Deepgram streaming implementation (config opt-in) | C |
| T3 | Dual independent pipelines (inbound/outbound transcribed concurrently, no head-of-line blocking) | C |
| T4 | Per-segment metadata: timestamps, confidence, measured latency | C |
| T5 | Automatic punctuation/casing on finals | C |
| T6 | First-run model downloader with checksums + progress UI | C |
| T7 | Custom vocabulary / bias list (product names, jargon — fed from RAG corpus terms) | S |
| T8 | Speaker diarization *within* the inbound stream (multi-party calls) | D |
| T9 | Live translation mode | D |

### 4.3 UI Layer

| # | Feature | Tier |
|---|---|---|
| U1 | Dual-column live transcript — inbound left, outbound right — with partials rendering in-place then solidifying | C |
| U2 | Smart auto-scroll (follows live edge; pauses when user scrolls up; "jump to live" pill) | C |
| U3 | Session lifecycle: start/stop/pause listening; sessions list; reopen past sessions | C |
| U4 | AI assist surface: select any message(s) → "Ask AI"; global hotkey for "assist on last exchange"; streamed answer cards | C |
| U5 | RAG library screen: drag-drop upload, ingest progress, per-doc enable/disable toggle, delete | C |
| U6 | In-session transcript search + full-text search across past sessions | C |
| U7 | Settings: devices, ASR engine/model, API keys, theme, hotkeys, privacy (cloud on/off) | C |
| U8 | Export session (Markdown / JSON) | C |
| U9 | Always-on-top compact mode ("sidecar" strip next to the call window) | S |
| U10 | Latency HUD (per-stage pipeline timings, live) | S |

### 4.4 RAG / Vector Layer

| # | Feature | Tier |
|---|---|---|
| R1 | Ingest PDF / DOCX / MD / TXT / HTML → clean text + structure-aware chunking (300–500 tokens, 15% overlap, heading breadcrumbs in metadata) | C |
| R2 | Local embeddings (BGE-small via fastembed), background worker, incremental (re-ingest only changed docs) | C |
| R3 | LanceDB store with hybrid retrieval: vector ANN + BM25 full-text, reciprocal-rank fusion | C |
| R4 | Retrieval API used by the orchestrator: `retrieve(query, k=8)` in <15 ms, with source attribution (doc, page/section) | C |
| R5 | "Peek" UI: every AI answer shows which chunks grounded it; click-through to the source doc | C |
| R6 | Reranker stage (cross-encoder, ONNX) for precision on large corpora | S |
| R7 | Ingest past convasist sessions into the corpus (see §6.4) | S |
| R8 | Folder watch (auto-ingest a synced directory) | D |

### 4.5 AI Orchestration Layer

| # | Feature | Tier |
|---|---|---|
| O1 | Context builder: rolling transcript window (both streams, interleaved by time) + top-k RAG chunks + session metadata, under a strict token budget | C |
| O2 | Manual assist: on-demand, scoped to selection / last exchange / whole session; streaming render | C |
| O3 | Prompt template library (answer suggestion, explain jargon, summarize-so-far, draft follow-up) — user-editable | C |
| O4 | Model routing: sonnet for on-demand, haiku for proactive/cheap paths; per-request override | C |
| O5 | Proactive triggers (see §6) with debounce + cooldown so suggestions never flood | S |
| O6 | Tool-use loop (let the model call `retrieve` itself for multi-hop questions) | D |

---

## 5. UI/UX & aesthetics

### 5.1 Design principles

1. **The transcript is the app.** Everything else (AI, RAG, settings) is summoned and dismissed; the conversation surface never yields center stage.
2. **Glanceable during a live call.** The user is *talking to someone* — they get ~1 second of attention per glance. Big type for live text, muted everything else, zero decorative noise.
3. **Motion communicates state, sparingly.** Partial text shimmers subtly in reduced opacity and *settles* (opacity 100%, weight up) when finalized — the user learns to trust "settled = accurate" without reading twice. No other animation on the hot path.
4. **Keyboard-first.** `Ctrl+Space` assist-on-last-exchange, `Ctrl+L` jump-to-live, `Ctrl+K` command palette, `Ctrl+F` search. Mouse optional during a call.

### 5.2 Layout

```
┌────────────────────────────────────────────────────────────────┐
│ ● REC 00:14:32   Session: "Acme renewal call"    [⏸] [■] [⚙]  │ ← status bar (thin)
├──────────────────────────────┬─────────────────────────────────┤
│  THEM (system audio)         │            YOU (microphone)     │
│                              │                                 │
│ ┌──────────────────────┐     │                                 │
│ │ …and honestly the    │     │      ┌───────────────────────┐  │
│ │ renewal price feels  │     │      │ Totally hear you — let│  │
│ │ high compared to…    │     │      │ me walk you through   │  │
│ └──────────────────────┘     │      │ what changed this year│  │
│   10:32:04                   │      └───────────────────────┘  │
│                              │                    10:32:11     │
│ ╭╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╮      │                                 │
│ ┆ so what would it     ┆     │   ← partial: dashed, 60% opacity│
│ ╰╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╯      │                                 │
├──────────────────────────────┴─────────────────────────────────┤
│ ✦ AI  «They're anchoring on price — your RAG notes say the     │ ← assist dock
│    2026 tier includes the SLA they asked about in March …»     │   (collapsible)
│    sources: pricing-2026.pdf §2 · acme-notes.md                 │
├────────────────────────────────────────────────────────────────┤
│ [🎙 −38dB ▂▃▅] [🔊 −22dB ▂▅▇]  whisper base.en · 420ms · ● ok  │ ← health strip
└────────────────────────────────────────────────────────────────┘
```

- **Two columns, chat-bubble alignment** — inbound left-aligned, outbound right-aligned, single shared timeline (a merged chronological rail is one toggle away for reading back).
- **Assist dock** on the bottom (not a right rail): suggestion cards stream in here, each with source attributions and one-click copy. Collapsed = a single ✦ line showing the latest suggestion headline.
- **Sidecar mode [S]:** the same UI reflows to a 380 px always-on-top strip — transcript + assist dock only — designed to sit beside a full-screen Zoom window.

### 5.3 Visual language — three directions (recommendation: a)

| | Direction | Palette & type | Feel |
|---|---|---|---|
| **a** | **"Operator"** (recommended) | Near-black slate `#0B1220` base, panel `#111A2B`; inbound accent **cyan** `#22D3EE`, outbound accent **violet** `#A78BFA`, AI accent **amber** `#F59E0B`. UI type: Geist/Inter; timestamps + latency: JetBrains Mono | Mission-control calm. The two per-stream accent hues do the spatial work — even in peripheral vision you know *who* is talking without reading |
| b | "Paper" | Warm off-white, ink text, terracotta/teal accents, serif headers | Beautiful for transcript *review*, but light UIs glare during video calls in dim rooms |
| c | "Radar" | Pure black OLED, single amber accent, scanline texture | Striking, but one accent color gives up the inbound/outbound color-coding that carries principle 5.1(2) |

Dark mode is the **primary** design target (calls happen next to dark video-call windows; the app must not be the brightest thing on screen). Light mode ships as a derived token set, same as the atomicapps CSS-variable model: all colors as CSS variables, semantic aliases only, no hex in components.

Accessibility floor: AA contrast for body text at both modes, `role="status"`/`aria-live` on streaming regions, full keyboard operability, reduced-motion mode disables the settle animation.

---

## 6. AI-driven enhancements — proposals for Phase 1

Four candidates, ranked. Each is a policy on top of the O-layer — none changes the architecture, which is exactly why they're worth deciding now (the trigger/debounce plumbing O5 is shared).

### 6.1 Live Answer Assist ⭐ recommended
When the inbound stream finalizes an utterance that VAD marks as a *question or objection directed at you*, haiku drafts 1–2 bullet talking points — grounded in RAG — before you've finished inhaling. Debounced (≥4 s between suggestions), cooldown-aware, and silent unless confidence is high. This is the "wow" feature: the app answers the question you were *about to be asked to answer*.

### 6.2 Question Radar + instant RAG cards
Lighter-weight sibling of 6.1: detect questions/asks in the inbound stream and show the **top RAG chunks verbatim** (no LLM call, <15 ms, zero cost) as a card — "your pricing doc, §2, says…". LLM elaboration is then one click. Ships even if 6.1's proactive LLM spend is deferred; same trigger plumbing.

### 6.3 Commitment & entity tracker
A haiku pass over finalized segments extracts and pins to a side panel: names, companies, dollar amounts, dates, deadlines, and *commitments* ("I'll send the contract Friday"). At session end these become the action-item list. Solves the universal "what did I promise?" problem and costs almost nothing (batched, one call per ~30 s of speech).

### 6.4 Conversation memory (sessions become RAG corpus)
Every finished session is summarized, chunked, and ingested into LanceDB alongside uploaded documents. Next call with the same person/company, the context builder retrieves *"last time, they objected to the SLA terms"* automatically. This compounds: convasist gets smarter every conversation. (R7 is the ingestion half; this adds the auto-summarize + entity-link half.)

**Recommendation:** ship **6.2 + 6.3** in Phase 1 core (cheap, high utility), **6.1** as the Phase 1 stretch flagship, **6.4** as Phase 1.5 — it needs a few weeks of accumulated sessions to shine anyway.

---

## 7. Risks & open questions

| # | Risk / question | Impact | Mitigation / decision needed |
|---|---|---|---|
| 1 | **Recording-consent law.** California (and ~10 other states) requires *all-party consent* to record/intercept a conversation. convasist transcribes both sides by design. | Legal — highest severity | Phase 1 must ship a consent posture: first-run consent acknowledgment, visible REC indicator, and easy pause. Owner decision on positioning (personal note-taking tool vs. anything marketed for covert use — the latter is a hard no). Not legal advice; worth 30 min with counsel before public release. |
| 2 | Echo/crosstalk: open speakers leak inbound audio into the mic → duplicated transcripts | UX quality | Phase 1: headset-recommended banner + simple cross-correlation suppression; A7 (AEC3) as stretch |
| 3 | Local ASR accuracy vs. latency (whisper base vs. small vs. cloud) | Core UX | The engine trait + a built-in A/B latency HUD make this an empirical, per-machine choice instead of a bet |
| 4 | GPU absence on low-end machines | Perf | CPU fallback auto-selects `tiny.en`/distil; settings surface the tradeoff honestly; Deepgram opt-in as the escape hatch |
| 5 | Cloud privacy boundary (Deepgram audio, Claude text) | Trust | Default = fully local ASR; every cloud toggle is opt-in, labeled with exactly what leaves the machine |
| 6 | Model download size (~150 MB–1 GB) on first run | Onboarding | T6 downloader with resume + checksums; `tiny.en` bootstrap so the app works in <1 min while better models fetch |
| 7 | Windows-first vs. macOS timing | Scope | Recommend **Windows-only Phase 1** (owner's daily platform; loopback is simplest there), macOS in Phase 1.5 behind the `AudioSource` trait |

---

## 8. Phase 1 milestone plan

| M | Deliverable | Proves |
|---|---|---|
| M0 | Tauri scaffold, CI (fmt/clippy/tsc/build), typed IPC skeleton, model downloader | The plumbing |
| M1 | Dual capture + VU meters + device hot-swap (no ASR yet) | The hard OS integration, de-risked first |
| M2 | Streaming transcription → dual-column live UI (partials/finals), session persistence | The core experience |
| M3 | Manual AI assist (U4/O1–O4) with streaming cards | The "assistant" in conversation assistant |
| M4 | RAG: ingest UI + hybrid retrieval wired into assist, source attribution | Grounded answers |
| M5 | Enhancements 6.2 + 6.3, latency HUD, export, polish, consent UX | Ship candidate |

Each milestone is independently demoable; M1 is deliberately the riskiest slice pulled earliest.

---

## 9. Decisions requested from the owner

| # | Decision | Options | Recommendation |
|---|---|---|---|
| 1 | Approve shell/stack | Tauri 2 / Electron+Rust / other | **Tauri 2** (§2.2) |
| 2 | Default ASR posture | Local whisper / cloud Deepgram / trait+both | **Trait + local default, cloud opt-in** (§2.3) |
| 3 | LLM provider | Claude API / local LLM / both | **Claude API streaming** (local LLM deferred) |
| 4 | Phase 1 OS scope | Windows-only / Win+macOS | **Windows-only**, macOS 1.5 (§7.7) |
| 5 | Visual direction | a Operator / b Paper / c Radar (§5.3) | **a — Operator** |
| 6 | Enhancement picks | §6.1–6.4 | **6.2 + 6.3 core, 6.1 stretch, 6.4 next** |
| 7 | Consent posture (§7.1) | acknowledge-on-first-run + REC indicator | Required before any external user |

Reply with row numbers (e.g., "1–6 approved as recommended, 7 discuss") — coding starts only after explicit approval per the behavioral rules.
