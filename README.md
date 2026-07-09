# Orchestrator

Slot-based model delegation: main agents (Claude Code, Codex) call a stable MCP `delegate` tool; you hot-swap the worker backend behind a named **slot** from a tray-resident GUI without restarting the MCP session.

## Status

| Milestone | Status |
|-----------|--------|
| **M1** Headless MCP core | Complete (audited) |
| **M2** Tauri GUI | Complete (audited) |
| **M3** CLIProxyAPI subscriptions | Complete (audited) |
| **M4** Resilience & local models | Complete |
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

## M3 — CLIProxyAPI (subscription OAuth)

Pinned release: **v7.2.58** (see `src-tauri/binaries/VERSION.txt`). Windows x64 binary only for now; Linux/Mac download URLs are noted in that file.

```powershell
# One-time: download sidecar (not vendored in git — ~47MB)
powershell -File scripts\download-cliproxy.ps1
```

- Sidecar lifecycle in `src-tauri/src/sidecar/` (spawn / health / restart backoff / kill on exit)
- Config + auth under `%AppData%\orchestrator\cliproxy\` (not `~/.cli-proxy-api`)
- Listen: `127.0.0.1:18317` (avoids clashing with a system CLIProxyAPI on 8317)
- GUI **Accounts**: enable sidecar, OAuth connect (claude/codex/antigravity/kimi/xai), disconnect, sync profiles
- Connected accounts become ordinary `sub-…` **openai_compatible** backend profiles → slot dropdown. Core never imports CLIProxyAPI types.
- Failures (sidecar down, auth, quota) → `worker unavailable: …` via existing adapter mapping

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

## M4 � Resilience & local models

- Ollama discovery (localhost:11434 + extra hosts) ? one-click openai_compatible profiles
- Tray notification on `worker unavailable` (debounced: 1/slot/min)
- Per-slot fallback chain GUI with **explicit opt-in** (still off by default)
- Usage JSONL log under app config dir with size rotation + recent activity UI
