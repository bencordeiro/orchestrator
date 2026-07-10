// Backend profiles: saved endpoints (local, API-key, subscription) + Ollama discovery.
import { useState } from 'react'
import { api } from '../lib/api'
import type { BackendProfileView, OllamaModel } from '../lib/api'
import { Empty, Pill, Section } from './ui'

type Props = {
  profiles: BackendProfileView[]
  notify: (msg: string, isError?: boolean) => void
  refresh: () => Promise<void>
}

export function BackendsView({ profiles, notify, refresh }: Props) {
  // manual profile form
  const [showForm, setShowForm] = useState(false)
  const [id, setId] = useState('')
  const [label, setLabel] = useState('')
  const [backend, setBackend] = useState('openai_compatible')
  const [baseUrl, setBaseUrl] = useState('')
  const [model, setModel] = useState('')
  const [authRef, setAuthRef] = useState('')
  const [secret, setSecret] = useState('')

  // ollama
  const [hosts, setHosts] = useState('')
  const [found, setFound] = useState<OllamaModel[] | null>(null)
  const [scanning, setScanning] = useState(false)

  async function save() {
    if (!id.trim() || !baseUrl.trim() || !model.trim()) {
      notify('Profile needs id, base URL and model', true)
      return
    }
    try {
      if (authRef.trim() && secret.trim()) {
        await api.setSecret(authRef.trim(), secret.trim())
      }
      await api.upsertProfile({
        id: id.trim(),
        label: label.trim() || id.trim(),
        backend,
        baseUrl: baseUrl.trim(),
        model: model.trim(),
        authRef: authRef.trim() || null,
      })
      notify(`Saved profile ${id.trim()}${secret ? ' (key stored in OS keychain)' : ''}`)
      setId(''); setLabel(''); setBaseUrl(''); setModel(''); setAuthRef(''); setSecret('')
      setShowForm(false)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function remove(p: BackendProfileView) {
    if (!confirm(`Remove backend profile "${p.label}"? Slots using it keep working until reassigned.`)) return
    try {
      await api.removeProfile(p.id)
      notify(`Removed profile ${p.label}`)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function scan() {
    setScanning(true)
    try {
      const extra = hosts.split(/[\n,]+/).map((s) => s.trim()).filter(Boolean)
      await api.setOllamaHosts(extra)
      const models = await api.discoverOllama()
      setFound(models)
      notify(models.length ? `Found ${models.length} local model(s)` : 'No Ollama models found — is Ollama running?')
    } catch (e) {
      notify(String(e), true)
    } finally {
      setScanning(false)
    }
  }

  async function adopt(m: OllamaModel) {
    try {
      const pid = await api.createOllamaProfile(m.host, m.name)
      notify(`Created profile ${pid} for ${m.name}`)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  const kind = (p: BackendProfileView) =>
    p.id.startsWith('sub-') ? 'subscription' : p.base_url.includes('11434') ? 'local' : p.auth_ref ? 'api key' : 'open'

  return (
    <>
      <Section
        title="Backend profiles"
        subtitle="Saved endpoints you can assign to any slot. Subscriptions sync automatically from Accounts."
        actions={
          <button className="btn btn-accent" onClick={() => setShowForm((v) => !v)}>
            {showForm ? 'Cancel' : '+ Add manually'}
          </button>
        }
      >
        {showForm && (
          <div className="add-form add-form-grid">
            <input placeholder="id (e.g. zai-glm)" value={id} onChange={(e) => setId(e.target.value)} />
            <input placeholder="label (e.g. z.ai GLM-4.7)" value={label} onChange={(e) => setLabel(e.target.value)} />
            <select value={backend} onChange={(e) => setBackend(e.target.value)}>
              <option value="openai_compatible">OpenAI-compatible</option>
              <option value="anthropic">Anthropic API</option>
            </select>
            <input placeholder="base URL (e.g. https://api.z.ai/api/coding/paas/v4)" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
            <input placeholder="model id" value={model} onChange={(e) => setModel(e.target.value)} />
            <input placeholder="auth ref (keychain name, optional)" value={authRef} onChange={(e) => setAuthRef(e.target.value)} />
            <input
              placeholder="API key (stored in OS keychain, optional)"
              type="password"
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
            />
            <button className="btn btn-accent" onClick={save}>Save profile</button>
          </div>
        )}

        {profiles.length === 0 ? (
          <Empty icon="▤">No profiles yet. Add one manually, scan Ollama below, or connect a subscription.</Empty>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Label</th>
                <th>Type</th>
                <th>Model</th>
                <th>Endpoint</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {profiles.map((p) => (
                <tr key={p.id}>
                  <td>{p.label}</td>
                  <td>
                    <Pill tone={kind(p) === 'subscription' ? 'accent' : kind(p) === 'local' ? 'ok' : 'neutral'}>
                      {kind(p)}
                    </Pill>
                  </td>
                  <td className="mono">{p.model}</td>
                  <td className="mono dim">{p.base_url}</td>
                  <td>
                    <button className="btn btn-ghost btn-sm" onClick={() => remove(p)}>✕</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>

      <Section
        title="Local models (Ollama)"
        subtitle="Scan localhost and any extra hosts; adopt models as backend profiles with one click."
        actions={
          <button className="btn" onClick={scan} disabled={scanning}>
            {scanning ? 'Scanning…' : 'Scan'}
          </button>
        }
      >
        <div className="ollama-hosts">
          <input
            placeholder="extra hosts, comma-separated (e.g. http://10.0.0.10:11434)"
            value={hosts}
            onChange={(e) => setHosts(e.target.value)}
          />
        </div>
        {found !== null &&
          (found.length === 0 ? (
            <Empty icon="◎">Nothing found. Ollama runs on localhost:11434 by default.</Empty>
          ) : (
            <ul className="found-list">
              {found.map((m) => (
                <li key={`${m.host}/${m.name}`}>
                  <span className="mono">{m.name}</span>
                  <span className="dim mono">{m.host}</span>
                  <button className="btn btn-sm btn-accent" onClick={() => adopt(m)}>
                    Add as profile
                  </button>
                </li>
              ))}
            </ul>
          ))}
      </Section>
    </>
  )
}
