// Connection commands, app options, updates, server details.
import { useState } from 'react'
import {
  disable as disableAutostart,
  enable as enableAutostart,
} from '@tauri-apps/plugin-autostart'
import { api } from '../lib/api'
import type { McpSetupCommands, ServerInfo, UpdateCheck } from '../lib/api'
import { CopyField, Section, Toggle } from './ui'

type Props = {
  server: ServerInfo | null
  commands: McpSetupCommands | null
  autostart: boolean
  setAutostart: (v: boolean) => void
  notify: (msg: string, isError?: boolean) => void
}

export function SettingsView({ server, commands, autostart, setAutostart, notify }: Props) {
  const [update, setUpdate] = useState<UpdateCheck | null>(null)
  const [checking, setChecking] = useState(false)

  async function toggleAutostart(on: boolean) {
    try {
      if (on) await enableAutostart()
      else await disableAutostart()
      setAutostart(on)
      notify(on ? 'Start on login enabled' : 'Start on login disabled')
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function check() {
    setChecking(true)
    try {
      setUpdate(await api.checkUpdates())
    } catch (e) {
      notify(String(e), true)
    } finally {
      setChecking(false)
    }
  }

  async function install() {
    try {
      notify(await api.installUpdate())
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function openLogs() {
    try {
      await api.openLogDir()
    } catch (e) {
      notify(String(e), true)
    }
  }

  return (
    <>
      <Section
        title="Connect your agents"
        subtitle="Run once per machine. Workers stay swappable afterwards — no re-setup ever."
      >
        {commands ? (
          <div className="stack">
            <CopyField label="Claude Code" value={commands.claude} />
            <CopyField label="Codex CLI" value={commands.codex} />
          </div>
        ) : (
          <p className="dim">Loading…</p>
        )}
      </Section>

      <Section title="App">
        <div className="stack">
          <Toggle checked={autostart} onChange={toggleAutostart} label="Start Orchestrator on login" />
          <div className="row update-row">
            <button className="btn" onClick={check} disabled={checking}>
              {checking ? 'Checking…' : 'Check for updates'}
            </button>
            {update && (
              <span className="dim">
                {update.message}
                {update.available && (
                  <button className="btn btn-accent btn-sm" style={{ marginLeft: 8 }} onClick={install}>
                    Install {update.latest_version}
                  </button>
                )}
              </span>
            )}
          </div>
          <div className="row">
            <button className="btn" onClick={openLogs}>
              Open logs folder
            </button>
            <span className="dim">Diagnostics for troubleshooting — share these if something misbehaves.</span>
          </div>
        </div>
      </Section>

      {server && (
        <Section title="Server" subtitle="Localhost only. Bearer-token protected; secrets live in the OS keychain.">
          <div className="stack">
            <CopyField label="MCP endpoint" value={server.mcp_url} />
            <CopyField label="Health" value={server.health_url} />
            <CopyField label="Config" value={server.config_path} />
          </div>
        </Section>
      )}
    </>
  )
}
