# convasist

A real-time AI conversation assistant: intercepts both sides of the host computer's audio (microphone + system output), transcribes them live into a dual-column chat UI, and lets a RAG-grounded AI agent process the conversation inline at any moment.

**Design blueprint:** [`docs/phase-1-design-and-spec.md`](docs/phase-1-design-and-spec.md) — tech stack, module boundaries, latency budgets, milestones, and the resolved decision checklist. Read it before touching code.

## Stack

Tauri 2 shell · Rust core (WASAPI capture → whisper.cpp ASR → LanceDB RAG → provider-agnostic LLM streaming, Claude default) · React 19 + TypeScript + Tailwind 4 UI. Windows is the Phase 1 target.

## Layout

```
convasist/
├── docs/                    # Blueprints, specs, phase docs
├── crates/convasist-core/   # Shell-agnostic domain layer: types, traits, IPC contract.
│                            # No GUI/OS deps — builds and tests on any platform.
├── src-tauri/               # Tauri 2 shell: wires UI ↔ core, platform implementations
├── src/                     # React UI (Operator theme, dual-column transcript)
└── models/                  # gitignored; ASR/embedding models fetched on first run
```

## Development (Windows)

Prereqs: [Rust](https://rustup.rs), Node 22+, [Tauri 2 Windows prerequisites](https://tauri.app/start/prerequisites/) (WebView2 is preinstalled on Windows 11).

```powershell
npm install
npm run tauri dev
```

UI-only iteration (no Rust shell, browser tab with empty states): `npm run dev`.

## Checks

| What | Command |
|---|---|
| Core lint + tests (any OS) | `cargo fmt --check` · `cargo clippy -p convasist-core --all-targets` · `cargo test -p convasist-core` |
| UI typecheck + build | `npm run build` |
| Tauri shell | `cargo clippy -p convasist-app --all-targets` (needs the UI built first) |

CI runs all three on every PR (`.github/workflows/ci.yml`); the shell job runs on `windows-latest`.

## Status

Phase 1, milestone **M4 (RAG engine)** built: drag-drop / file-picker document ingestion (PDF, DOCX, MD, TXT, HTML) → structure-aware chunking with heading breadcrumbs (`convasist-core/src/chunk.rs`) → persistent per-document store under app-data with BM25 retrieval (`convasist-core/src/bm25.rs`, `src-tauri/src/rag.rs`) wired into every assist: top-8 chunks ground the prompt and each answer card shows its sources (R5 "peek"). The Library panel manages documents (enable/disable toggle, delete). The vector/embedding half of hybrid retrieval (fastembed + ANN + reciprocal-rank fusion) is the next layer behind the same `RagStore` seam.

Earlier: **M3 (manual AI assist)** — the assist dock's Suggest reply (`Ctrl+Space`) / Summarize / free-form question actions build a budgeted context from the finalized transcript (`convasist-core/src/prompt.rs`, unit-tested) and stream the answer as cards via `ASSIST_CHUNK` events. Provider clients (`src-tauri/src/llm.rs`): Anthropic native (default), an OpenAI-compatible adapter (OpenAI / xAI / DeepSeek / local Ollama), and Gemini — all SSE-normalized; keys live in the OS credential vault (`keyring`), with per-provider Save/Test (measured first-token latency) and live model lists merged into the §4.6 dropdowns.

Earlier: **M2 (streaming transcription)** — each side's 16 kHz frames flow through an energy-VAD utterance segmenter (`convasist-core/src/vad.rs`, unit-tested) into a per-side whisper.cpp engine (`src-tauri/src/asr.rs`) sharing one loaded model. Partials re-decode the open utterance every ~1.2 s (greedy) and stream to the UI as replaceable segments; silence-close finalizes with a small beam. The ggml model auto-downloads on first start with progress events (`src-tauri/src/models.rs`). M1 delivered the dual capture (cpal/WASAPI loopback, VU meters, watchdog, hot-swap, consent gate); M0 the workspace + typed IPC contract (`crates/convasist-core/src/ipc.rs` mirrored by `src/lib/ipc.ts` — change both in the same commit) + provider registry with Claude default.

Real capture + transcription require Windows (loopback is WASAPI-only) — pending hands-on validation there. Next: **M5 — Phase 1 enhancements + polish** (Question Radar, commitment tracker, latency HUD, export — design §6/§8), plus the embedding upgrade to hybrid retrieval.
