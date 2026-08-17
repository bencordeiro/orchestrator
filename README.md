# Orchestrator

**One delegation tool. Any model. Switch live.**

Orchestrator is a local MCP server and tray app that gives Claude Code, Codex,
and other MCP clients stable AI worker roles. Your client delegates to a name
such as `worker` or `reviewer`; you decide which model and provider currently
fills that role—and can change it without reconnecting the client or losing the
conversation.

![Orchestrator slot management interface](docs/orchestrator-ui.png)

## Why Orchestrator?

AI clients normally couple a workflow to one provider or model. Changing it can
mean editing configuration, restarting MCP, and teaching the main agent about
vendor-specific details. Orchestrator puts a small, local routing layer between
the client and its workers:

1. **Connect once** — your MCP client gets a stable `delegate` tool.
2. **Delegate by role** — send work to named slots such as `worker` or `reviewer`.
3. **Choose the backend yourself** — assign Ollama, Anthropic, or any
   OpenAI-compatible endpoint from the tray app.
4. **Switch live** — the next call uses the new backend while existing
   conversation history remains available.

The client only needs to know what each slot is for. Provider names, model IDs,
credentials, and routing decisions stay inside Orchestrator.

## How it works

```
Claude Code · Codex · any MCP client
                 │
          delegate("worker")
                 │
                 ▼
        Orchestrator on localhost
          │              │
     worker slot     reviewer slot
          │              │
          ▼              ▼
   Ollama / API      Anthropic / proxy
```

## Features

- **MCP over streamable HTTP** on localhost (server outlives client sessions)
- **Two tools only:** `delegate` and `list_slots` (list never reveals vendor/model)
- **Call-time slot resolve** — edit config or GUI; next `delegate` uses the new backend
- **Conversation continuity** — history lives in Orchestrator, so threads survive slot swaps
- **Backends:** any OpenAI-compatible URL (Ollama, proxies, …) + native Anthropic — see [docs/BACKENDS.md](docs/BACKENDS.md) for recipes (z.ai coding plan, Ollama, API keys)
- **Agent onboarding:** copy-paste setup + usage prompt for any MCP client — [AGENT_SETUP.md](AGENT_SETUP.md)
- **Optional CLIProxyAPI sidecar** for subscription OAuth → local OpenAI-compatible API
- **Tray app** (Windows + Linux): slot board, accounts, Ollama discovery, usage log, manual update check

## Quick start (Windows)

### Installer (release)

1. Download the latest **NSIS** installer from [GitHub Releases](https://github.com/bencordeiro/orchestrator/releases).
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
git clone https://github.com/bencordeiro/orchestrator.git
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

## Quick start (Linux)

Packages: **`.deb`** (Debian/Ubuntu/Pop!_OS) and **`.AppImage`** (any distro).
Only the AppImage supports the in-app updater — Tauri cannot update a `.deb`.

```bash
sudo apt install ./Orchestrator_<version>_amd64.deb
# or
chmod +x Orchestrator_<version>_amd64.AppImage && ./Orchestrator_<version>_amd64.AppImage
```

Then connect a backend and copy the MCP setup command from **Settings →
Connect your agents**, exactly as on Windows.

> **Tray icon:** Orchestrator lives in the tray. GNOME needs the
> [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/);
> on COSMIC enable the tray applet in panel settings. Without a tray host the
> app still runs and serves MCP — closing the window hides it, and relaunching
> brings it back.

### From source (dev)

Build prerequisites (Ubuntu/Debian/Pop!_OS — the Tauri v2 set, plus
`libdbus-1-dev` for the Secret Service keyring backend):

```bash
sudo apt update && sudo apt install -y \
  libwebkit2gtk-4.1-dev libssl-dev librsvg2-dev \
  libxdo-dev libayatana-appindicator3-dev libdbus-1-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

```bash
git clone https://github.com/bencordeiro/orchestrator.git
cd orchestrator
./scripts/download-cliproxy.sh    # optional, for subscription sidecar
(cd ui && npm ci)
cargo test
# GUI:
(cd ui && npm run tauri dev)
# Headless MCP only:
cargo run -- serve
```

**One-command release build** (tests + UI + deb/AppImage):

```bash
./scripts/release.sh
```

Packages land under `src-tauri/target/release/bundle/`.

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
| `scripts/download-cliproxy.ps1` | Download pinned CLIProxyAPI (checksummed), Windows x64 |
| `scripts/download-cliproxy.sh` | Download pinned CLIProxyAPI (checksummed), Linux/macOS |
| `scripts/release.ps1` | Full test + UI build + Tauri NSIS/MSI + optional `/health` smoke |
| `scripts/release.sh` | Full test + UI build + Tauri deb/AppImage + optional `/health` smoke |
| `.github/workflows/ci.yml` | Test on push (Windows + Linux matrix); release artifacts on tag |

The sidecar pin and per-platform checksums live in
`src-tauri/binaries/VERSION.txt`. Both download scripts read it, so adding a
target is a data change — append a `<platform>_url` / `_sha256` / `_binary` /
`_triple` group, no script edits.

**Future platforms:** macOS is not built yet (needs a Gatekeeper/signing
decision); the sidecar ships darwin builds upstream, so it is mostly packaging.

## License

MIT — see [LICENSE](LICENSE). Third-party notices: [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Updater keys

Manual “Check for updates” only. Signing keys: [docs/UPDATER.md](docs/UPDATER.md).
