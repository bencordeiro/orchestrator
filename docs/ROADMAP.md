# Roadmap to 1.0

Versioning policy: **+0.0.1 per significant change.** `1.0.0` ships only when
we agree the product is polished. This file is the running ledger of what
stands between here and there — add candidates as dogfooding surfaces them,
strike items as they land.

## Reliability & correctness (highest priority)

- [ ] **Startup failure → visible error dialog.** Today a bad config or bound
  port panics and the app silently never appears (the BOM incident). Users
  must see *why* it didn't start.
- [ ] **Persist background jobs to disk** (or at minimum mark orphaned ids).
  In-memory jobs die silently with the process; a killed app should leave
  `job_result` able to say "lost to restart — re-delegate" instead of
  "unknown id".
- [ ] **Job cancellation**: `cancel_job(job_id)` MCP tool + cancel button in
  the GUI job list. Today a runaway/mistaken background job can only be
  stopped by killing the whole app — which kills every other job with it.
  Abort the in-flight backend request; mark the job Failed("cancelled").
- [ ] **Adopt or kill orphaned sidecar on startup.** A force-killed app leaves
  `cli-proxy-api.exe` running, which blocks reinstall (file lock) and can
  serve stale auth. Detect on boot; reattach or terminate.
- [ ] **Config path independent of working directory.** Launching the exe from
  a repo/dev cwd silently picks up a different slots.json. Resolve config
  strictly from the OS app-config dir unless an explicit `--config` flag is
  passed.
- [ ] **Bump pinned CLIProxyAPI sidecar** (v7.2.58 → v7.2.77+, checksummed) and
  re-verify OAuth flows for all five providers.
- [ ] Conversation auto-expiry (threads currently accumulate forever).
- [ ] Rust warning sweep (unused imports etc.) + clippy pass.

## UX / UI polish

- [ ] **In-flight job indicator**: slot card shows "generating…" plus a live
  background-jobs list (ids, elapsed, state) — the GUI is blind to activity
  today until the usage log updates.
- [ ] **Connect-agents panel: present Codex setup as a config-file edit**
  (paste into `~/.codex/config.toml`) with its own copy button — it is not a
  shell command like the Claude one, and the current single "commands" list
  confuses non-experts.
- [ ] In-app setup guide (bundle AGENT_SETUP.md content) — friends receiving
  the exe don't have the repo.
- [ ] Startup-at-login toggle surfaced in Settings (plugin exists; UI unclear).
- [ ] First-run onboarding: point at tray icon (users think the app "closed"),
  offer to create the first slot + connect an account.

## Features (post-validation, pre-1.0 candidates)

- [ ] Per-slot system prompts (user idea): give a slot standing instructions
  server-side, invisible to the main model.
- [ ] `list_jobs` MCP tool so agents can enumerate their own running jobs
  after context loss (GUI-only today).
- [ ] Usage dashboard: per-slot/backend token + latency stats from
  usage.jsonl rendered in Activity tab.
- [ ] Optional per-slot max-concurrency cap: N simultaneous requests per
  backend before new delegations queue, so an agent fanning out background
  jobs can't trip provider rate limits. Off by default (unlimited), user-set
  only — consistent with the no-automatic-behavior invariant.
- [ ] Bearer-token rotation button in Settings that regenerates the token
  and re-renders both setup snippets — rotating today means manually
  editing every client config (the token-confusion saga).
- [ ] **Pause toggle (tray + GUI)**: one click stops accepting delegations —
  tools return a clear "workers paused by user" instead of executing.
  Motivated by dogfooding: globally-registered MCP + global agent
  instructions means *every* session in every project may delegate;
  sometimes the user wants that off without editing client configs.

## Providers

Current OAuth lane (via CLIProxyAPI): Claude, OpenAI/Codex, Gemini +
Antigravity, Kimi, xAI/Grok — all exposed. API-key lane recipes documented in
BACKENDS.md (z.ai coding plan, Ollama, generic OpenAI-compatible).

- [ ] Watch CLIProxyAPI releases for new OAuth providers; add to the Accounts
  UI provider list when they appear.
- [ ] More curated day-one model ids per provider as they ship (KNOWN_MODELS
  in AccountsView).

## Distribution & launch checklist

- [ ] Pick the public, searchable project name (repo + app + installer).
- [ ] Replace `YOUR_GITHUB_USER` in tauri.conf.json updater endpoint.
- [ ] Create GitHub repo, push, tag, add TAURI_SIGNING_PRIVATE_KEY(+PASSWORD)
  secrets; CI release workflow already exists.
- [ ] Code signing (SmartScreen currently warns on the unsigned exe).
- [ ] Clean-machine install test (no dev tools present).
- [ ] Demo GIF for the README; README pitch: *"Not another multi-agent
  framework — one stable worker tool with a swappable back end. Two models,
  zero ceremony."*
- [ ] Announce: r/LocalLLaMA, r/ClaudeAI, Show HN, MCP directories.

## Platform

- [ ] Linux + macOS builds via GitHub Actions matrix (Tauri makes this mostly
  config; sidecar binaries exist for all three). macOS needs a
  Gatekeeper/signing decision.

## Done (recent)

- [x] 0.1.10 — File-based logging: rotating daily log (7-file cap) under the app
  config dir alongside stderr, plus an "Open logs folder" button in Settings.
  Installed apps have no console; this is the diagnostic trail the "it doesn't
  run" saga lacked. Fails safe (stderr-only) if the file can't be opened.
- [x] 0.1.9 — Codex native streamable HTTP setup (npx/mcp-remote shim removed
  from product), BOM-tolerant config, single-instance guard, versioning
  policy adopted.
- [x] v0.2 background jobs (`delegate(background=true)` + `job_result`,
  6h retention) — live-validated from Claude Code and Codex.
- [x] release.ps1 never kills a running app for its smoke test.
