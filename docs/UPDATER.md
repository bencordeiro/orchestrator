# Updater signing keys (Windows)

Orchestrator uses [Tauri’s updater](https://v2.tauri.app/plugin/updater/) with a **manual** “Check for updates” button only — **no background auto-update in v1**.

## Public key

The **public** key is embedded in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.

## Private key (NEVER commit)

Generate once on a secure machine:

```powershell
# Creates ~/.tauri/orchestrator/orchestrator.key (+ .pub)
mkdir $env:USERPROFILE\.tauri\orchestrator -Force
cd ui
npx tauri signer generate -w "$env:USERPROFILE\.tauri\orchestrator\orchestrator.key" --ci -f
```

**Private key location (local maintainer machine):**

```
%USERPROFILE%\.tauri\orchestrator\orchestrator.key
```

This path is **outside the git repo**. The repo `.gitignore` also blocks `*.key` and `.tauri/`.

### CI / release signing

Set one of:

| Variable | Meaning |
|----------|---------|
| `TAURI_SIGNING_PRIVATE_KEY` | Full private key string |
| `TAURI_SIGNING_PRIVATE_KEY_PATH` | Path to the key file |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Optional password |

`scripts/release.ps1` and `.github/workflows/ci.yml` expect these secrets on **tag** builds.

## Endpoints

`tauri.conf.json` points at:

```
https://github.com/YOUR_GITHUB_USER/orchestrator/releases/latest/download/latest.json
```

Replace `YOUR_GITHUB_USER` when the public repo exists. Tauri’s release action (or a manual upload of `latest.json` + signed artifacts) must publish that file on each GitHub Release.

## If you lose the private key

You must generate a new keypair, ship a new app build with the new **public** key, and re-sign future updates. Existing installs with the old pubkey cannot verify new packages.
