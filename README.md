# Orchestrator

**Hot-swappable worker slots for any MCP client** (Claude Code, Codex CLI, and friends).

Your main agent keeps one stable tool — `delegate` — while **you** decide which model/backend sits behind the `worker` (or `reviewer`, …) slot. Swap backends mid-session from a tray app **without** restarting MCP and **without** the main model needing to know the vendor.

```
Claude Code / Codex  --MCP HTTP-->  Orchestrator (tray)  --OpenAI/Anthropic-->  worker backends
                                         │
                                    slots.json (hot reload)
                                    Ollama · API keys · CLIProxyAPI subscriptions
```

![Demo placeholder](docs/demo.gif)

*Add a short screen recording as `docs/demo.gif` before public launch.*

## Features

- **MCP over streamable HTTP** on localhost (server outlives client sessions)
- **Two tools only:** `delegate` and `list_slots` (list never reveals vendor/model)
- **Call-time slot resolve** — edit config or GUI; next `delegate` uses the new backend
- **Conversation continuity** — history lives in Orchestrator, so threads survive slot swaps
- **Backends:** any OpenAI-compatible URL (Ollama, proxies, …) + native Anthropic — see [docs/BACKENDS.md](docs/BACKENDS.md) for recipes (z.ai coding plan, Ollama, API keys)
- **Agent onboarding:** copy-paste setup + usage prompt for any MCP client — [AGENT_SETUP.md](AGENT_SETUP.md)
- **Optional CLIProxyAPI sidecar** for subscription OAuth → local OpenAI-compatible API
- **Tray app** (Windows): slot board, accounts, Ollama discovery, usage log, manual update check

## Quick start (Windows)

### Installer (release)

1. Download the latest **NSIS** installer from [GitHub Releases](https://github.com/YOUR_GITHUB_USER/orchestrator/releases).
2. Install and launch **Orchestrator** (tray icon appears; config + bearer token auto-created on first run).
3. Connect a backend:
   - **Ollama:** Local models → Discover → Create profile → assign to `worker`
   - **API key:** add a backend profile + store the key via the secrets/keychain flow
   - **Subscription:** Accounts → enable sidecar → OAuth (optional; see ToS risk below)
4. Copy the MCP setup command from the UI, or run:

```powershell
claude mcp add --transport http orchestrator http://localhost:7420/mcp `
  --header "Authorization: Bearer <token-from-app>"
```

5. In Claude Code: ask it to `delegate` a task.

### From source (dev)

```powershell
git clone https://github.com/YOUR_GITHUB_USER/orchestrator.git
cd orchestrator
powershell -File scripts\download-cliproxy.ps1   # optional, for subscription sidecar
cd ui; npm install; cd ..
cargo test
# GUI:
cd ui; npm run tauri dev
# Headless MCP only:
cargo run -- serve
```

**One-command release build** (tests + UI + NSIS/MSI):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release.ps1
```

Installers land under `src-tauri\target\release\bundle\`.

## Slots & continuity

| Concept | Behavior |
|---------|----------|
| **Slot** | Stable name (`worker`, …) with a capability description |
| **Backend** | Swappable: model + base URL + auth ref (or subscription profile) |
| **Hot-swap** | GUI or `slots.json` edit → next `delegate` (no MCP restart) |
| **Fresh job** | Omit `conversation_id` |
| **Continued thread** | Pass prior `conversation_id`; history is replayed from disk |
| **Opacity** | `list_slots` never exposes vendor/model — only name + description |
| **Failures** | `worker unavailable: <reason>` — **no automatic** slot switching |
| **Fallback** | Optional chain, **off by default**, explicit opt-in in the GUI |

## Subscription ToS risk (read this)

Some backends can be wired through **subscription OAuth relays** (e.g. CLIProxyAPI). Providers have blocked or restricted third-party use of consumer subscriptions before.

Orchestrator does **not** hide this:

- Subscription lanes are **fragile and user-controlled**.
- Prefer **API keys** and **local models (Ollama)** when you need reliability or compliance.
- You are responsible for complying with each provider’s terms of service.

## Security (summary)

- Localhost MCP + bearer token  
- Secrets in OS keychain  
- See [SECURITY.md](SECURITY.md)

## Building & CI

| Script | Purpose |
|--------|---------|
| `scripts/download-cliproxy.ps1` | Download pinned CLIProxyAPI (checksummed) for Windows x64 |
| `scripts/release.ps1` | Full test + UI build + Tauri NSIS/MSI + optional `/health` smoke |
| `.github/workflows/ci.yml` | Test on push; release artifacts on tag |

**Future platforms:** Linux/macOS installers are not in v1; sidecar download URLs for those OSes are listed in `src-tauri/binaries/VERSION.txt`.

## License

MIT — see [LICENSE](LICENSE). Third-party notices: [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Updater keys

Manual “Check for updates” only. Signing keys: [docs/UPDATER.md](docs/UPDATER.md).
