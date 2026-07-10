// Subscription accounts via the CLIProxyAPI sidecar, with per-account model override.
import { useEffect, useState } from 'react'
import { api } from '../lib/api'
import type { AccountsList, SidecarStatus } from '../lib/api'
import { Empty, Pill, Section, StatusDot, Toggle } from './ui'

type Props = {
  sidecar: SidecarStatus | null
  accountsList: AccountsList | null
  notify: (msg: string, isError?: boolean) => void
  refresh: () => Promise<void>
}

const PROVIDER_LABELS: Record<string, string> = {
  claude: 'Claude (blocked by Anthropic for relays — expect failures)',
  codex: 'ChatGPT / Codex',
  codex_device: 'ChatGPT / Codex (device code)',
  antigravity: 'Antigravity (Gemini)',
  kimi: 'Kimi',
  xai: 'Grok (xAI)',
}

export function AccountsView({ sidecar, accountsList, notify, refresh }: Props) {
  const [providers, setProviders] = useState<string[]>([])
  const [provider, setProvider] = useState('antigravity')
  const [models, setModels] = useState<string[]>([])
  const [pendingModel, setPendingModel] = useState<Record<string, string>>({})

  useEffect(() => {
    api.oauthProviders().then(setProviders).catch(() => {})
  }, [])

  useEffect(() => {
    if (accountsList?.state === 'ok') {
      api.proxyModels().then(setModels).catch(() => setModels([]))
    }
  }, [accountsList?.state])

  async function toggleSidecar(on: boolean) {
    try {
      await api.setSidecarEnabled(on)
      notify(on ? 'Subscriptions enabled — sidecar starting…' : 'Subscriptions disabled')
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function connect() {
    try {
      await api.startOAuth(provider)
      notify(`Browser opening for ${provider} sign-in. Come back and hit Sync when done.`)
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function sync() {
    try {
      const ids = await api.syncProfiles()
      notify(`Synced ${ids.length} subscription profile(s)`)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function disconnect(name: string) {
    if (!confirm(`Disconnect ${name}? Its stored login is removed.`)) return
    try {
      await api.disconnectAccount(name)
      await api.syncProfiles()
      notify(`Disconnected ${name}`)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function applyOverride(accountId: string) {
    const m = pendingModel[accountId]
    if (!m) return
    try {
      await api.setModelOverride(accountId, m)
      notify(`Model pinned to ${m}`)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function clearOverride(accountId: string) {
    try {
      await api.clearModelOverride(accountId)
      notify('Model override cleared — auto-pick restored')
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  const state = accountsList?.state ?? 'stopped'
  const running = sidecar?.running && sidecar?.healthy

  return (
    <Section
      title="Subscription accounts"
      subtitle="OAuth into provider subscriptions — no API keys. ToS-gray lane: providers can break this; keep a local/API-key backend ready to swap in."
      actions={
        <div className="row">
          <span className="sidecar-state">
            <StatusDot state={running ? 'ok' : sidecar?.enabled ? 'warn' : 'idle'} />
            {state === 'not_installed'
              ? 'relay not installed'
              : state === 'disabled'
                ? 'off'
                : running
                  ? 'relay running'
                  : 'starting…'}
          </span>
          {state !== 'not_installed' && (
            <Toggle checked={!!sidecar?.enabled} onChange={toggleSidecar} />
          )}
        </div>
      }
    >
      {state === 'not_installed' ? (
        <Empty icon="↓">
          The subscription relay (CLIProxyAPI) isn't installed. Run{' '}
          <code className="mono">scripts\download-cliproxy.ps1</code> and restart the app. Everything else works
          without it.
        </Empty>
      ) : state === 'disabled' ? (
        <Empty icon="⏻">Subscriptions are off. Flip the toggle to start the relay.</Empty>
      ) : (
        <>
          <div className="connect-row">
            <select value={provider} onChange={(e) => setProvider(e.target.value)}>
              {(providers.length ? providers : Object.keys(PROVIDER_LABELS)).map((p) => (
                <option key={p} value={p}>
                  {PROVIDER_LABELS[p] ?? p}
                </option>
              ))}
            </select>
            <button className="btn btn-accent" onClick={connect} disabled={!running}>
              Connect account
            </button>
            <button className="btn" onClick={sync} disabled={!running}>
              Sync
            </button>
          </div>

          {accountsList && accountsList.accounts.length === 0 ? (
            <Empty icon="⚿">{accountsList.message || 'No accounts connected yet.'}</Empty>
          ) : (
            <ul className="account-list">
              {accountsList?.accounts.map((a) => (
                <li key={a.id} className="account-card">
                  <div className="account-main">
                    <StatusDot state={a.unavailable ? 'err' : a.disabled ? 'idle' : 'ok'} />
                    <div>
                      <strong>{a.provider}</strong>
                      <span className="dim"> · {a.email ?? a.label ?? a.name}</span>
                      {a.status_message && <div className="dim account-msg">{a.status_message}</div>}
                    </div>
                    <Pill tone={a.unavailable ? 'err' : 'ok'}>{a.status || 'active'}</Pill>
                  </div>
                  <div className="account-actions">
                    <select
                      value={pendingModel[a.id] ?? ''}
                      onChange={(e) => setPendingModel((m) => ({ ...m, [a.id]: e.target.value }))}
                    >
                      <option value="">pin model…</option>
                      {models.map((m) => (
                        <option key={m} value={m}>
                          {m}
                        </option>
                      ))}
                    </select>
                    <button className="btn btn-sm" onClick={() => applyOverride(a.id)} disabled={!pendingModel[a.id]}>
                      Pin
                    </button>
                    <button className="btn btn-ghost btn-sm" onClick={() => clearOverride(a.id)}>
                      Auto
                    </button>
                    <button className="btn btn-ghost btn-sm" onClick={() => disconnect(a.name)}>
                      Disconnect
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </Section>
  )
}
