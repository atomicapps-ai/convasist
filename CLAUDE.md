# CLAUDE.md — convasist

> Brief for AI assistants (and humans) working in this repo. Read this, then
> [`README.md`](README.md) for the run guide and [`docs/phase-1-design-and-spec.md`](docs/phase-1-design-and-spec.md)
> for the full design (architecture, latency budgets, milestones, §9 decisions).

## What convasist is

A real-time AI conversation assistant. It intercepts **both** sides of the host
computer's audio — your microphone (outbound) and the system output / other
party (inbound, via WASAPI loopback) — transcribes them live into a dual-column
chat UI, and lets a RAG-grounded LLM process the conversation inline at any
moment. It can also record the live call to a stereo WAV (you = left, them =
right) via `src-tauri/src/recorder.rs` — a background writer thread fed by the
existing capture frames, so recording adds no work to the audio or UI path.
Windows is the Phase 1 target (loopback is WASAPI-only).

## Stack

Tauri 2 shell (Rust core + system WebView) · React 19 + TypeScript + Tailwind 4 +
Zustand UI · cpal/WASAPI capture → whisper.cpp ASR (whisper-rs; opt-in Deepgram
cloud streaming via `asr_engine=deepgram_cloud` + key, `src-tauri/src/asr_deepgram.rs`)
→ hybrid RAG (BM25 + fastembed/ONNX embeddings, RRF fusion) → provider-agnostic
LLM streaming (Anthropic default; OpenAI/Google/xAI/DeepSeek/Ollama). **Do not
swap a layer without asking the owner.**

## Repo layout

| Path | What |
|---|---|
| `docs/` | Blueprints/specs — design doc lives here; new design docs go here. |
| `crates/convasist-core/` | Shell-agnostic domain layer: types, traits, IPC contract, pure logic (DSP, VAD, chunking, BM25, RRF, prompt/tracker/radar). **Builds + tests on any OS** — this is where unit tests live. |
| `src-tauri/` | Tauri 2 shell: platform implementations (audio, ASR, models, LLM clients, RAG store, sessions, tracker) + the `#[tauri::command]` surface. |
| `src/` | React UI. `lib/ipc.ts` mirrors the Rust IPC contract; `lib/commands.ts` wraps the Tauri commands; `state/*` is Zustand. |
| `models/` | gitignored; ASR + embedding models auto-download on first run. |

## Architecture rules — do not break

1. **Core stays platform-agnostic.** `crates/convasist-core` has no GUI/OS deps.
   Anything touching cpal/whisper/keyring/tauri/fs lives in `src-tauri`. Pure,
   testable logic belongs in core (and gets a unit test there).
2. **The IPC contract is mirrored by hand.** `crates/convasist-core/src/ipc.rs`
   (Rust) ↔ `src/lib/ipc.ts` (TypeScript). Change one, change the other **in the
   same commit**. Events are namespaced `convasist://*`.
3. **Every Tauri command has a typed wrapper** in `src/lib/commands.ts` and its
   types in `src/lib/ipc.ts`. Adding a command = update both sides.
4. **Audio threading contract (§2.4).** The cpal device callback ONLY copies
   samples into the lock-free rtrb ring — no allocation, locks, or logging. A
   dedicated worker drains, downmixes, resamples to 16 kHz mono, and hands off
   `AudioFrame`s. Never do work in the callback.
5. **Blocking I/O off the UI/audio path.** LLM streaming and model downloads use
   blocking `ureq` on dedicated threads / `spawn_blocking`, never the UI thread.
6. **API keys live in the OS credential vault** (`keyring`) at runtime, never in
   plaintext files/config. Empty submission clears the key. They may optionally
   be exported to a **passphrase-encrypted** file (`*.secrets.enc`, cocoon) that
   is safe to commit to git and travels to another machine; the passphrase comes
   from the `CONVASIST_SECRETS_PASSPHRASE` env var (never committed), and on
   startup missing keys are seeded from that file. See `src-tauri/src/secrets.rs`.
7. **RAG is best-effort hybrid.** Retrieval fuses BM25 + cosine (RRF) and
   **degrades to BM25-only** when the embedder isn't ready — hybrid is an
   upgrade, never a hard dependency. Ingestion supports pdf/docx/md/txt/html
   plus pasted text (stored as `.txt`).

## Build & run (Windows)

Full prerequisites + a build-troubleshooting table (real failures: libclang,
the LLVM-20 layout assert → **pin LLVM 18.1.8**, stdbool.h/stdio.h, cmake) are
in [`README.md`](README.md). Short version, from a fresh terminal after the
prereqs:

```
npm install
npm run tauri dev      # first launch downloads the whisper + embedding models
```

Local whisper runs on CPU by default. For conversation-speed transcription,
build with the GPU backend (Vulkan SDK prereq + `npm run tauri dev -- --features
gpu-vulkan`) — see README "GPU-accelerated whisper". The log line
`[asr] whisper backend: …` tells you which backend a running build uses.

Default settings live in the repo-committed `convasist.config.json` — a fresh
machine seeds its config from it (Settings → "Export settings…" writes the
current values back for committing). LLM API keys are NEVER in that file —
they are entered in-app (Settings). To carry keys to
another machine, set `CONVASIST_SECRETS_PASSPHRASE` (any strong passphrase),
Settings → **Export encrypted…**, commit the resulting `convasist.secrets.enc`,
then on the other machine set the same env var and the keys load on startup.
Currently the app lives only on branch `claude/convasist-architecture-design-aoh6o5`
(PR #1) — on a fresh clone, `git checkout` that branch before building until it
merges.

## Checks (run before pushing)

| What | Command |
|---|---|
| Core lint + tests (any OS) | `cargo fmt --check` · `cargo clippy -p convasist-core --all-targets` · `cargo test -p convasist-core` |
| Shell tests + lint (Windows) | `cargo test -p convasist-app` · `cargo clippy -p convasist-app --all-targets` |
| UI typecheck + build | `npm run build` |

CI (`.github/workflows/ci.yml`) runs core lint+test on ubuntu, UI typecheck+build
on ubuntu, and the shell clippy `-D warnings` on windows-latest. Clippy runs with
`-D warnings` — keep it clean.

## Workflow

- Develop on the assigned feature branch; don't commit to `main` locally.
- Commit/push only when the owner asks. Keep the IPC Rust↔TS mirror and the
  command wrappers in lockstep within a commit.
- Prefer adding pure logic to core with a unit test over untested shell code.
