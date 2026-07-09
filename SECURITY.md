# Security

## Threat model (v1)

Orchestrator is a **localhost-only** control plane for AI coding tools:

- The MCP HTTP server binds to `127.0.0.1` by default (not a public host).
- Access is protected by a **bearer token** stored in the OS keychain (and generated on first launch if missing).
- Worker API keys and subscription proxy keys also live in the **OS keychain** via the `keyring` crate — not in `slots.json`.
- CLIProxyAPI (when enabled) is bound to `127.0.0.1` with its own management key under the app config directory.

## What this does *not* do

- No multi-user auth
- No remote/host-mode MCP by design in v1
- No automatic model routing or hidden provider calls beyond the backends **you** configure

## Reporting issues

If you find a security issue (for example, accidental binding to `0.0.0.0`, secret leakage into logs, or path traversal in conversation IDs), please open a private security advisory on the GitHub repository once published, or contact the maintainer listed on the repo.

## Subscription / ToS note

Using subscription OAuth proxies may violate provider terms of service. Orchestrator treats those backends as **user-controlled, fragile lanes** — prefer API keys and local models when in doubt. See the README “Subscription ToS risk” section.
