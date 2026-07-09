import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from '@tauri-apps/plugin-autostart'
import './App.css'

type SlotBoardItem = {
  name: string
  description: string
  backend_kind: string
  base_url: string
  model: string
  auth_ref: string | null
  profile_id: string | null
  profile_label: string | null
  enable_fallback: boolean
  fallback: string[]
  last_call_at: string | null
  last_latency_ms: number | null
  last_error: string | null
  last_success: boolean | null
}

type OllamaModel = {
  name: string
  host: string
  openai_base_url: string
}

type UsageEvent = {
  ts: string
  slot: string
  profile_id?: string | null
  base_url: string
  model: string
  latency_ms: number
  success: boolean
  reason?: string | null
}

type BackendProfileView = {
  id: string
  label: string
  backend: string
  base_url: string
  model: string
  auth_ref: string | null
}

type ServerInfo = {
  listen: string
  mcp_url: string
  health_url: string
  config_path: string
}

type SidecarStatus = {
  enabled: boolean
  running: boolean
  healthy: boolean
  version_pin: string
  base_url: string
  openai_base_url: string
  port: number
  binary_path: string
  config_path: string
  last_error: string | null
  restart_count: number
}

type AuthAccount = {
  id: string
  name: string
  provider: string
  email: string | null
  label: string | null
  status: string
  status_message: string
  unavailable: boolean
  disabled: boolean
}

function App() {
  const [slots, setSlots] = useState<SlotBoardItem[]>([])
  const [profiles, setProfiles] = useState<BackendProfileView[]>([])
  const [server, setServer] = useState<ServerInfo | null>(null)
  const [mcpCmd, setMcpCmd] = useState('')
  const [autostart, setAutostart] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [statusMsg, setStatusMsg] = useState<string | null>(null)

  // Add-slot form
  const [newName, setNewName] = useState('')
  const [newDesc, setNewDesc] = useState('')
  const [newProfile, setNewProfile] = useState('')

  // Add-profile form
  const [profId, setProfId] = useState('')
  const [profLabel, setProfLabel] = useState('')
  const [profBackend, setProfBackend] = useState('openai_compatible')
  const [profUrl, setProfUrl] = useState('')
  const [profModel, setProfModel] = useState('')
  const [profAuth, setProfAuth] = useState('')
  const [sidecar, setSidecar] = useState<SidecarStatus | null>(null)
  const [accounts, setAccounts] = useState<AuthAccount[]>([])
  const [oauthProviders, setOauthProviders] = useState<string[]>([])
  const [oauthProvider, setOauthProvider] = useState('claude')
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([])
  const [extraHosts, setExtraHosts] = useState('')
  const [usage, setUsage] = useState<UsageEvent[]>([])

  const refresh = useCallback(async () => {
    try {
      const [board, pro, info, cmd, sc, acc, prov, hosts, recent] = await Promise.all([
        invoke<SlotBoardItem[]>('get_slot_board'),
        invoke<BackendProfileView[]>('get_backend_profiles'),
        invoke<ServerInfo>('get_server_info'),
        invoke<string>('get_mcp_setup_command'),
        invoke<SidecarStatus>('get_sidecar_status'),
        invoke<AuthAccount[]>('list_subscription_accounts'),
        invoke<string[]>('list_oauth_providers'),
        invoke<string[]>('get_ollama_extra_hosts'),
        invoke<UsageEvent[]>('get_recent_usage', { limit: 40 }),
      ])
      setSlots(board)
      setProfiles(pro)
      setServer(info)
      setMcpCmd(cmd)
      setSidecar(sc)
      setAccounts(acc)
      setOauthProviders(prov)
      setExtraHosts(hosts.join('\n'))
      setUsage(recent)
      setError(null)
      if (pro.length && !newProfile) {
        setNewProfile(pro[0].id)
      }
    } catch (e) {
      setError(String(e))
    }
  }, [newProfile])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 3000)
    isAutostartEnabled()
      .then(setAutostart)
      .catch(() => setAutostart(false))
    return () => clearInterval(t)
  }, [refresh])

  async function onSwap(slot: string, profileId: string) {
    try {
      await invoke('swap_slot_backend', { slotName: slot, profileId })
      setStatusMsg(`Swapped "${slot}" → ${profileId} (force_reload; next delegate uses new backend)`)
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  async function onRemove(slot: string) {
    if (!confirm(`Remove slot "${slot}"?`)) return
    try {
      await invoke('remove_slot', { name: slot })
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  async function onAddSlot() {
    try {
      await invoke('add_slot', {
        args: { name: newName, description: newDesc, profileId: newProfile },
      })
      setNewName('')
      setNewDesc('')
      setStatusMsg(`Added slot "${newName}"`)
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  async function onAddProfile() {
    try {
      await invoke('upsert_backend_profile', {
        args: {
          id: profId,
          label: profLabel,
          backend: profBackend,
          baseUrl: profUrl,
          model: profModel,
          authRef: profAuth || null,
        },
      })
      setProfId('')
      setProfLabel('')
      setProfUrl('')
      setProfModel('')
      setProfAuth('')
      setStatusMsg(`Saved backend profile "${profId}"`)
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  async function copyMcp() {
    try {
      await navigator.clipboard.writeText(mcpCmd)
      setStatusMsg('MCP setup command copied to clipboard')
    } catch {
      setError('Clipboard write failed — select the command and copy manually')
    }
  }

  async function toggleAutostart() {
    try {
      if (autostart) {
        await disableAutostart()
        setAutostart(false)
        setStatusMsg('Start on login disabled')
      } else {
        await enableAutostart()
        setAutostart(true)
        setStatusMsg('Start on login enabled')
      }
    } catch (e) {
      setError(String(e))
    }
  }

  async function toggleSidecar() {
    if (!sidecar) return
    try {
      await invoke('set_sidecar_enabled', { enabled: !sidecar.enabled })
      setStatusMsg(
        !sidecar.enabled
          ? 'CLIProxyAPI sidecar enabled — starting…'
          : 'CLIProxyAPI sidecar disabled',
      )
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  async function connectOAuth() {
    try {
      await invoke('start_subscription_oauth', { provider: oauthProvider })
      setStatusMsg(
        `OAuth flow started for ${oauthProvider} (browser should open). Click Refresh accounts after login.`,
      )
    } catch (e) {
      setError(String(e))
    }
  }

  async function disconnectAccount(name: string) {
    if (!confirm(`Disconnect account ${name}?`)) return
    try {
      await invoke('disconnect_subscription_account', { name })
      await invoke('sync_subscription_profiles')
      setStatusMsg(`Disconnected ${name}`)
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  async function syncProfiles() {
    try {
      const ids = await invoke<string[]>('sync_subscription_profiles')
      setStatusMsg(`Synced ${ids.length} subscription profile(s) into backend dropdown`)
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  async function discoverOllama() {
    try {
      const hosts = extraHosts
        .split(/[\n,]+/)
        .map((s) => s.trim())
        .filter(Boolean)
      await invoke('set_ollama_extra_hosts', { hosts })
      const models = await invoke<OllamaModel[]>('discover_ollama_models')
      setOllamaModels(models)
      setStatusMsg(
        models.length
          ? `Found ${models.length} Ollama model(s)`
          : 'No Ollama models found (is Ollama running on localhost:11434?)',
      )
    } catch (e) {
      setError(String(e))
    }
  }

  async function createOllamaProfile(m: OllamaModel) {
    try {
      const id = await invoke<string>('create_ollama_profile', { host: m.host, model: m.name })
      setStatusMsg(`Created backend profile "${id}" — available in slot dropdown`)
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  async function saveFallback(slot: SlotBoardItem, enable: boolean, chain: string) {
    try {
      const fallback = chain
        .split(/[,\s]+/)
        .map((s) => s.trim())
        .filter(Boolean)
      await invoke('set_slot_fallback', {
        args: { name: slot.name, enableFallback: enable, fallback },
      })
      setStatusMsg(
        enable
          ? `Fallback enabled for "${slot.name}" → ${fallback.join(' → ') || '(empty chain)'}`
          : `Fallback disabled for "${slot.name}" (default)`,
      )
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  function formatTime(iso: string | null) {
    if (!iso) return '—'
    try {
      return new Date(iso).toLocaleString()
    } catch {
      return iso
    }
  }

  return (
    <div className="app">
      <header className="header">
        <div>
          <h1>Orchestrator</h1>
          <p className="sub">Slot-based model delegation · MCP hot-swap board</p>
        </div>
        <div className="header-actions">
          <button type="button" className="btn secondary" onClick={() => refresh()}>
            Refresh
          </button>
          <button type="button" className="btn" onClick={copyMcp}>
            Copy MCP setup command
          </button>
        </div>
      </header>

      {error && <div className="banner error">{error}</div>}
      {statusMsg && (
        <div className="banner ok" onClick={() => setStatusMsg(null)}>
          {statusMsg}
        </div>
      )}

      <section className="panel">
        <h2>Server</h2>
        {server && (
          <dl className="kv">
            <dt>MCP</dt>
            <dd>
              <code>{server.mcp_url}</code>
            </dd>
            <dt>Health</dt>
            <dd>
              <code>{server.health_url}</code>
            </dd>
            <dt>Config</dt>
            <dd>
              <code>{server.config_path}</code>
            </dd>
          </dl>
        )}
        <label className="check">
          <input type="checkbox" checked={autostart} onChange={toggleAutostart} />
          Start on login
        </label>
        <div className="mcp-cmd">
          <label>Claude Code setup</label>
          <textarea readOnly rows={3} value={mcpCmd} />
        </div>
      </section>

      <section className="panel">
        <h2>Accounts (CLIProxyAPI subscriptions)</h2>
        {sidecar && (
          <div className="sidecar-meta">
            <label className="check">
              <input type="checkbox" checked={sidecar.enabled} onChange={toggleSidecar} />
              Run subscription sidecar (CLIProxyAPI {sidecar.version_pin})
            </label>
            <dl className="kv">
              <dt>Status</dt>
              <dd>
                {sidecar.healthy ? (
                  <span className="ok-text">healthy</span>
                ) : sidecar.running ? (
                  <span className="err-text">running, not healthy</span>
                ) : sidecar.enabled ? (
                  <span className="err-text">not running</span>
                ) : (
                  'disabled'
                )}
                {sidecar.restart_count > 0 && ` · restarts: ${sidecar.restart_count}`}
              </dd>
              <dt>OpenAI URL</dt>
              <dd>
                <code>{sidecar.openai_base_url}</code>
              </dd>
              <dt>Config</dt>
              <dd>
                <code>{sidecar.config_path}</code>
              </dd>
            </dl>
            {sidecar.last_error && <pre className="err-box">{sidecar.last_error}</pre>}
          </div>
        )}
        <div className="oauth-row">
          <label className="field">
            Provider
            <select value={oauthProvider} onChange={(e) => setOauthProvider(e.target.value)}>
              {(oauthProviders.length ? oauthProviders : ['claude', 'codex', 'antigravity', 'kimi', 'xai']).map(
                (p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ),
              )}
            </select>
          </label>
          <button type="button" className="btn" onClick={connectOAuth}>
            Connect subscription (OAuth)
          </button>
          <button type="button" className="btn secondary" onClick={syncProfiles}>
            Sync profiles to dropdown
          </button>
        </div>
        <ul className="profile-list">
          {accounts.map((a) => (
            <li key={a.id}>
              <div className="card-head">
                <strong>
                  {a.provider} · {a.email ?? a.label ?? a.name}
                </strong>
                <button type="button" className="btn danger sm" onClick={() => disconnectAccount(a.name)}>
                  Disconnect
                </button>
              </div>
              <div className="meta">
                status:{' '}
                <span className={a.unavailable || a.status !== 'active' ? 'err-text' : 'ok-text'}>
                  {a.status}
                  {a.unavailable ? ' (unavailable)' : ''}
                </span>
                {a.status_message ? ` — ${a.status_message}` : ''}
                {a.disabled ? ' · disabled' : ''}
              </div>
            </li>
          ))}
          {accounts.length === 0 && (
            <p className="empty">
              No subscription accounts connected. Enable the sidecar and run OAuth, or sync after login.
            </p>
          )}
        </ul>
        <p className="meta">
          Connected accounts auto-register as <code>sub-…</code> backend profiles (OpenAI-compatible → local
          CLIProxyAPI). Slot/delegate path stays generic — no special-casing.
        </p>
      </section>

      <section className="panel">
        <h2>Local models (Ollama)</h2>
        <p className="meta">
          Probes <code>http://localhost:11434</code> plus optional extra hosts. Creates ordinary{' '}
          <code>openai_compatible</code> profiles (no core special-casing).
        </p>
        <label className="field">
          Extra hosts (one per line)
          <textarea
            rows={2}
            value={extraHosts}
            onChange={(e) => setExtraHosts(e.target.value)}
            placeholder="http://192.168.1.20:11434"
          />
        </label>
        <button type="button" className="btn" onClick={discoverOllama}>
          Discover Ollama models
        </button>
        <ul className="profile-list">
          {ollamaModels.map((m) => (
            <li key={`${m.host}-${m.name}`}>
              <div className="card-head">
                <strong>{m.name}</strong>
                <button type="button" className="btn sm" onClick={() => createOllamaProfile(m)}>
                  Create profile
                </button>
              </div>
              <div className="meta">
                {m.host} → <code>{m.openai_base_url}</code>
              </div>
            </li>
          ))}
          {ollamaModels.length === 0 && (
            <p className="empty">No models discovered yet — click Discover.</p>
          )}
        </ul>
      </section>

      <section className="panel">
        <h2>Slot board</h2>
        <div className="cards">
          {slots.map((s) => (
            <article key={s.name} className="card">
              <div className="card-head">
                <h3>{s.name}</h3>
                <button type="button" className="btn danger sm" onClick={() => onRemove(s.name)}>
                  Remove
                </button>
              </div>
              <p className="desc">{s.description}</p>
              <div className="backend">
                <span className="label">Backend</span>
                <strong>{s.profile_label ?? `${s.model} @ ${s.base_url}`}</strong>
                <div className="meta">
                  <code>{s.backend_kind}</code> · <code>{s.model}</code>
                  <br />
                  <code className="url">{s.base_url}</code>
                </div>
              </div>
              <label className="field">
                Swap backend
                <select
                  value={s.profile_id ?? ''}
                  onChange={(e) => {
                    if (e.target.value) onSwap(s.name, e.target.value)
                  }}
                >
                  <option value="" disabled>
                    Select profile…
                  </option>
                  {profiles.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label} ({p.id})
                    </option>
                  ))}
                </select>
              </label>
              <div className="fallback-box">
                <label className="check">
                  <input
                    type="checkbox"
                    checked={!!s.enable_fallback}
                    onChange={(e) =>
                      saveFallback(s, e.target.checked, (s.fallback ?? []).join(', '))
                    }
                  />
                  Opt-in fallback chain (off by default)
                </label>
                <label className="field">
                  Fallback slot names (ordered, comma-separated)
                  <input
                    defaultValue={(s.fallback ?? []).join(', ')}
                    key={`${s.name}-fb-${(s.fallback ?? []).join(',')}`}
                    placeholder="reviewer, backup"
                    onBlur={(e) => saveFallback(s, !!s.enable_fallback, e.target.value)}
                  />
                </label>
              </div>
              <div className="status">
                <div>
                  <span className="label">Last call</span>
                  <div>{formatTime(s.last_call_at)}</div>
                </div>
                <div>
                  <span className="label">Latency</span>
                  <div>{s.last_latency_ms != null ? `${s.last_latency_ms} ms` : '—'}</div>
                </div>
                <div>
                  <span className="label">Status</span>
                  <div
                    className={
                      s.last_success === true
                        ? 'ok-text'
                        : s.last_success === false
                          ? 'err-text'
                          : ''
                    }
                  >
                    {s.last_success === true
                      ? 'OK'
                      : s.last_success === false
                        ? 'Error'
                        : 'Idle'}
                  </div>
                </div>
              </div>
              {s.last_error && <pre className="err-box">{s.last_error}</pre>}
            </article>
          ))}
          {slots.length === 0 && <p className="empty">No slots configured.</p>}
        </div>
      </section>

      <div className="grid-2">
        <section className="panel">
          <h2>Add slot</h2>
          <label className="field">
            Name
            <input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="reviewer" />
          </label>
          <label className="field">
            Description (MCP-visible)
            <input
              value={newDesc}
              onChange={(e) => setNewDesc(e.target.value)}
              placeholder="Code review worker"
            />
          </label>
          <label className="field">
            Backend profile
            <select value={newProfile} onChange={(e) => setNewProfile(e.target.value)}>
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </label>
          <button type="button" className="btn" onClick={onAddSlot} disabled={!newName || !newProfile}>
            Add slot
          </button>
        </section>

        <section className="panel">
          <h2>Backend profiles</h2>
          <ul className="profile-list">
            {profiles.map((p) => (
              <li key={p.id}>
                <strong>{p.label}</strong> <code>{p.id}</code>
                <div className="meta">
                  {p.backend} · {p.model}
                  <br />
                  {p.base_url}
                </div>
              </li>
            ))}
          </ul>
          <h3>Add / update profile</h3>
          <label className="field">
            Id
            <input value={profId} onChange={(e) => setProfId(e.target.value)} placeholder="local-qwen" />
          </label>
          <label className="field">
            Label
            <input value={profLabel} onChange={(e) => setProfLabel(e.target.value)} placeholder="Local Qwen" />
          </label>
          <label className="field">
            Kind
            <select value={profBackend} onChange={(e) => setProfBackend(e.target.value)}>
              <option value="openai_compatible">openai_compatible</option>
              <option value="anthropic">anthropic</option>
            </select>
          </label>
          <label className="field">
            Base URL
            <input
              value={profUrl}
              onChange={(e) => setProfUrl(e.target.value)}
              placeholder="http://localhost:11434/v1"
            />
          </label>
          <label className="field">
            Model
            <input value={profModel} onChange={(e) => setProfModel(e.target.value)} placeholder="qwen35b" />
          </label>
          <label className="field">
            Auth ref (keychain)
            <input
              value={profAuth}
              onChange={(e) => setProfAuth(e.target.value)}
              placeholder="worker_api_key"
            />
          </label>
          <button
            type="button"
            className="btn"
            onClick={onAddProfile}
            disabled={!profId || !profLabel || !profUrl || !profModel}
          >
            Save profile
          </button>
        </section>
      </div>

      <section className="panel">
        <h2>Recent activity (usage log)</h2>
        <p className="meta">
          JSONL under app config dir · success/failure, latency, reason · rotated when large. No token
          counting in v1.
        </p>
        <div className="usage-table">
          {usage.map((u, i) => (
            <div key={`${u.ts}-${i}`} className={`usage-row ${u.success ? 'ok' : 'err'}`}>
              <span className="usage-ts">{formatTime(u.ts)}</span>
              <span className="usage-slot">{u.slot}</span>
              <span className="usage-model">
                {u.model}
                {u.profile_id ? ` · ${u.profile_id}` : ''}
              </span>
              <span className="usage-lat">{u.latency_ms} ms</span>
              <span className="usage-res">{u.success ? 'OK' : u.reason ?? 'fail'}</span>
            </div>
          ))}
          {usage.length === 0 && <p className="empty">No usage events yet.</p>}
        </div>
      </section>

      <footer className="footer">
        Close window to hide to tray · Quit from tray menu · GUI mutations call force_reload() · Fallback
        chains opt-in only
      </footer>
    </div>
  )
}

export default App
