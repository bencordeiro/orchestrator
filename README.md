# Orchestrator

Slot-based model delegation: main agents (Claude Code, Codex) call a stable MCP `delegate` tool; you hot-swap the worker backend behind a named **slot** from a tray-resident GUI without restarting the MCP session.

## Status

| Milestone | Status |
|-----------|--------|
| **M1** Headless MCP core | Complete (audited) |
| **M2** Tauri GUI | Complete |
| M3 CLIProxyAPI | Not started |
| M4 Resilience / Ollama discovery | Not started |
| M5 Packaging | Not started |

## Architecture

- **Library** (`orchestrator` crate): slot registry, backends, conversations, MCP tools
- **Headless binary**: `cargo run -- serve` (M1)
- **Desktop app** (`src-tauri` + `ui/`): Tauri 2 + React/TS embeds the same library; MCP runs inside the process

## M1 — Headless core

```powershell
cargo run -- secrets set mcp_bearer_token "dev-token-change-me"
cargo run -- secrets set worker_api_key "YOUR_API_KEY"
cargo run -- serve slots.json
```

Tools: `delegate`, `list_slots` (opaque). Config: `slots.json`. Secrets: OS keychain. Slot resolved **at call time**.

```powershell
cargo test
```

## M2 — Tauri GUI

### Features

- Slot board: cards with name, description, assigned backend, last call / latency / error
- Add/remove slots; swap backend via dropdown (writes `slots.json` then **`force_reload()`**)
- Backend profiles for the dropdown
- System tray: show / hide / quit (close window hides to tray)
- Start on login (`tauri-plugin-autostart`)
- **Copy MCP setup command** (includes bearer token)

### Run (dev)

```powershell
# From repo root — ensures slots.json is found
$env:ORCHESTRATOR_SLOTS = "$PWD\slots.json"
$env:ORCHESTRATOR_BEARER_TOKEN = "dev-token-change-me"   # optional if keychain set

cd ui
npm install
npm run tauri dev
```

Or build + run:

```powershell
cd ui ; npm run build ; cd ..
cd src-tauri ; cargo build
# from repo root:
.\src-tauri\target\debug\orchestrator-app.exe
```

### GUI mutation contract

Every slot/profile change goes through `SlotRegistry::mutate` / `assign_backend` / `upsert_*`, which **write the config file and call `force_reload()`**. The next `delegate` uses the new backend without restart and without waiting on filesystem mtime.

## Config sketch

See `slots.example.json`. Optional `backend_profiles` feed the GUI dropdown; each slot still stores full backend fields (M1-compatible).

## Layout

```
src/                 # M1 library + headless binary
src-tauri/           # Tauri 2 host (embeds library, tray, commands)
ui/                  # React + TypeScript + Vite frontend
tests/               # integration tests (hot-swap, continuity, GUI mutate path)
slots.example.json
```
