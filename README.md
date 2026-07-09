# Orchestrator (M1 — headless core)

Slot-based model delegation MCP server. Main agents (Claude Code, Codex) call a stable `delegate` tool; you hot-swap the worker backend behind a named **slot** without restarting the MCP session.

## What M1 includes

- Standalone Rust crate: library + thin `orchestrator` binary (`cargo run`)
- MCP over **streamable HTTP** on localhost (`rmcp`), protected by a **bearer token**
- Tools:
  - `delegate(task, slot?, conversation_id?, context?, files?)` — default slot `worker`
  - `list_slots()` — names + capability descriptions only (never vendor/model)
- `slots.json` registry: slot → `{ backend, base_url, model, auth_ref, fallback }`
- Secrets in the **OS keychain** (`keyring`) — never plaintext in config
- Slot resolved **at call time** every `delegate` (hot-reload via mtime) — proven by tests
- Backends: OpenAI-compatible chat-completions (streaming) + native Anthropic
- Conversation history stored orchestrator-side on disk (survives slot swap mid-thread)
- No automatic slot switching; errors return `worker unavailable: <reason>`

## Quick start

```powershell
# Build
cargo build

# Secrets (OS keychain)
cargo run -- secrets set mcp_bearer_token "dev-token-change-me"
cargo run -- secrets set worker_api_key "YOUR_API_KEY"

# Or env overrides for first run:
#   $env:ORCHESTRATOR_BEARER_TOKEN = "dev-token-change-me"
#   $env:ORCHESTRATOR_WORKER_API_KEY = "YOUR_API_KEY"

# Serve
cargo run -- serve slots.json
```

Register with Claude Code:

```powershell
claude mcp add --transport http orchestrator http://127.0.0.1:7420/mcp `
  --header "Authorization: Bearer dev-token-change-me"
```

## Config (`slots.json`)

```json
{
  "listen": "127.0.0.1:7420",
  "bearer_token_ref": "mcp_bearer_token",
  "conversations_dir": "data/conversations",
  "slots": {
    "worker": {
      "description": "General-purpose coding and reasoning worker",
      "backend": "openai_compatible",
      "base_url": "http://10.0.0.10:8000/v1",
      "model": "qwen35b",
      "auth_ref": "worker_api_key",
      "enable_fallback": false
    }
  }
}
```

Edit `base_url` / `model` / `backend` and save — the **next** `delegate` call uses the new backend. No restart.

## Tests

```powershell
cargo test
```

Critical coverage:

- `tests/hot_swap.rs` — hot-swap takes effect on the next `delegate`
- `tests/conversation_continuity.rs` — history replay + continuity after mid-thread slot swap

## Layout

```
src/
  lib.rs           # library surface
  main.rs          # thin CLI (serve / secrets / init)
  config.rs        # slots.json schema + mtime reload
  registry.rs      # call-time slot resolve
  secrets.rs       # keyring + in-memory store for tests
  conversation.rs  # disk-persisted threads
  core.rs          # delegate / list_slots
  backends/        # openai_compatible + anthropic
  mcp/             # streamable HTTP + bearer auth
```

## Out of scope for M1

Tauri GUI (M2), CLIProxyAPI sidecar (M3), tray notifications (M4), packaging (M5).
