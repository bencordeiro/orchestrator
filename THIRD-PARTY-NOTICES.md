# Third-party notices

Orchestrator depends on and may **download/bundle** third-party software. This file summarizes licenses for components that are notable for distribution. Rust crate licenses are also declared in their respective `Cargo.toml` files on crates.io.

## CLIProxyAPI (bundled sidecar)

- **Project:** [router-for-me/CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)
- **Version (pinned):** 7.2.58 — see `src-tauri/binaries/VERSION.txt`
- **License:** MIT
- **Use in Orchestrator:** Optional managed sidecar for subscription OAuth → local OpenAI-compatible HTTP. Downloaded at release build time via `scripts/download-cliproxy.ps1` (SHA-256 verified) and bundled as a Tauri `externalBin` for Windows x64. **Not forked or modified.**

## Other runtime / UI stack

| Component | Role | License (typical) |
|-----------|------|-------------------|
| [Tauri](https://tauri.app/) | Desktop shell | MIT / Apache-2.0 |
| [rmcp](https://github.com/modelcontextprotocol/rust-sdk) | MCP protocol (Rust) | See crate |
| React, Vite | GUI frontend | MIT |
| Ollama (optional, user-installed) | Local models | Not bundled — user installs separately |

A full SBOM of Rust crates can be generated with `cargo tree` / `cargo license` during release.

## Future platforms

Linux and macOS CLIProxyAPI builds are **not** packaged in v1. Download URLs for those platforms are recorded in `src-tauri/binaries/VERSION.txt` for a later release.
