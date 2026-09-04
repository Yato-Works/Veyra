# Veyra

> **An AI operating layer for Windows that turns human intent into verified execution.**

Veyra is not a chatbot, a voice transcription app, or a Siri clone.
Veyra is the runtime that turns human intent into AI-powered, **verified** action on Windows.

See [`Plan.txt`](./Plan.txt) for the full technical design specification.

## Architecture

```text
Human Intent → Goal → Context → AI Selection → Planning
             → Execution → Verification → State Update → Human
```

Core principles:

- Conversation Context ≠ Goal
- Goal ≠ Project State
- Event Log = Historical Source of Truth (append-only)
- LLM ≠ Runtime (provider-independent)
- Generate ≠ Done (always Verify)
- Voice ≠ Product

## Repository Layout

```text
crates/
  veyra-core/     Domain types: Goal, Requirement, AcceptanceCriterion, Event, ProjectState, Decision
  veyra-store/    SQLite persistence, append-only Event Log, Projector (events -> state)
  veyra-runtime/  Runtime State Machine, Event Bus, Agent Orchestrator
  veyra-ai/       AI Gateway (provider-independent trait) and AI Router
  veyra-tools/    Tool trait, Tool Runtime, Permission levels
src-tauri/        Tauri 2 shell (planned, M2)
src/              React + TypeScript frontend (planned, M2)
```

## Development

```bash
cargo test        # run all Rust unit/integration tests
cargo build
```

## Roadmap

| Milestone | Scope |
|---|---|
| ✅ M1 | Core domain types, SQLite event store, projector, runtime state machine |
| ✅ M2 | Tauri 2 shell + minimal UI (projects, goals, decisions, feedback via IPC) |
| ✅ M3 | Text loop: intent (Ollama qwen2.5:1.5b + rule fallback) → goal → AI → event log |
| ✅ M4 | Verification engine (deterministic command/file/AI review) + bounded agent loop |
| M5 | Voice: Flow Mode (VAD + STT) & TTS via Style-Bert-VITS2 HTTP client |
| M6+ | Assistant Mode, Hermes, Windows automation, browser, calendar |

## Running the desktop app

```bash
npm install
npm run tauri dev    # dev mode with hot reload
npm run tauri build  # release bundle
```

