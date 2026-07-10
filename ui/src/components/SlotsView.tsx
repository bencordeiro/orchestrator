// Hero view: slot cards with inline swap — the thing you do most.
import { useState } from 'react'
import { api } from '../lib/api'
import type { BackendProfileView, SlotBoardItem } from '../lib/api'
import { Empty, Pill, Section, StatusDot, Toggle, timeAgo } from './ui'

type Props = {
  slots: SlotBoardItem[]
  profiles: BackendProfileView[]
  notify: (msg: string, isError?: boolean) => void
  refresh: () => Promise<void>
}

function slotState(s: SlotBoardItem): 'ok' | 'warn' | 'err' | 'idle' {
  if (s.last_success === null) return 'idle'
  return s.last_success ? 'ok' : 'err'
}

export function SlotsView({ slots, profiles, notify, refresh }: Props) {
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [desc, setDesc] = useState('')
  const [profileId, setProfileId] = useState('')
  const [busySlot, setBusySlot] = useState<string | null>(null)

  async function swap(slot: string, pid: string) {
    if (!pid) return
    setBusySlot(slot)
    try {
      await api.swapSlot(slot, pid)
      notify(`${slot} → ${profiles.find((p) => p.id === pid)?.label ?? pid}. Next delegate uses it.`)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    } finally {
      setBusySlot(null)
    }
  }

  async function toggleFallback(s: SlotBoardItem, on: boolean) {
    try {
      await api.setSlotFallback(s.name, on, s.fallback)
      notify(on ? `Fallback enabled for ${s.name}` : `Fallback disabled for ${s.name}`)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function remove(s: SlotBoardItem) {
    if (!confirm(`Remove slot "${s.name}"? Connected agents will get "unknown slot" for it.`)) return
    try {
      await api.removeSlot(s.name)
      notify(`Removed slot ${s.name}`)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  async function addSlot() {
    if (!name.trim() || !profileId) {
      notify('Slot needs a name and a backend', true)
      return
    }
    try {
      await api.addSlot(name.trim(), desc.trim() || 'Worker slot', profileId)
      notify(`Added slot ${name.trim()}`)
      setName('')
      setDesc('')
      setAdding(false)
      await refresh()
    } catch (e) {
      notify(String(e), true)
    }
  }

  return (
    <Section
      title="Slots"
      subtitle="Stable names your agents delegate to. Swap the backend live — connected sessions never notice."
      actions={
        <button className="btn btn-accent" onClick={() => setAdding((v) => !v)}>
          {adding ? 'Cancel' : '+ New slot'}
        </button>
      }
    >
      {adding && (
        <div className="add-form">
          <input placeholder="slot name (e.g. reviewer)" value={name} onChange={(e) => setName(e.target.value)} />
          <input
            placeholder="capability description shown to agents"
            value={desc}
            onChange={(e) => setDesc(e.target.value)}
          />
          <select value={profileId} onChange={(e) => setProfileId(e.target.value)}>
            <option value="">backend…</option>
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
          <button className="btn btn-accent" onClick={addSlot}>
            Create
          </button>
        </div>
      )}

      {slots.length === 0 ? (
        <Empty icon="◌">No slots yet. Create one and point your agent's delegate tool at it.</Empty>
      ) : (
        <div className="slot-grid">
          {slots.map((s) => (
            <article key={s.name} className={`slot-card slot-${slotState(s)}`}>
              <header className="slot-head">
                <div className="slot-title">
                  <StatusDot state={slotState(s)} pulse={busySlot === s.name} />
                  <h3>{s.name}</h3>
                </div>
                <button className="btn btn-ghost btn-sm slot-remove" onClick={() => remove(s)} title="Remove slot">
                  ✕
                </button>
              </header>

              <p className="slot-desc">{s.description}</p>

              <div className="slot-backend">
                <label>Backend</label>
                <select
                  value={s.profile_id ?? ''}
                  disabled={busySlot === s.name}
                  onChange={(e) => swap(s.name, e.target.value)}
                >
                  {!s.profile_id && <option value="">custom · {s.model}</option>}
                  {profiles.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </select>
                <div className="slot-model mono" title={s.base_url}>
                  {s.model}
                </div>
              </div>

              <footer className="slot-foot">
                <span className="slot-stat">
                  {s.last_call_at ? (
                    <>
                      {timeAgo(s.last_call_at)}
                      {s.last_latency_ms != null && <> · {s.last_latency_ms} ms</>}
                    </>
                  ) : (
                    'no calls yet'
                  )}
                </span>
                <span className="slot-flags">
                  {s.enable_fallback && <Pill tone="warn">fallback on</Pill>}
                  <Toggle checked={s.enable_fallback} onChange={(v) => toggleFallback(s, v)} label="fallback" />
                </span>
              </footer>
              {s.last_error && !s.last_success && <div className="slot-error mono">{s.last_error}</div>}
            </article>
          ))}
        </div>
      )}
    </Section>
  )
}
