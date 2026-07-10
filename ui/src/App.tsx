// Orchestrator control panel — dark, tray-resident, slots front and center.
import { useCallback, useEffect, useRef, useState } from 'react'
import { isEnabled as isAutostartEnabled } from '@tauri-apps/plugin-autostart'
import { api } from './lib/api'
import type {
  AccountsList,
  BackendProfileView,
  McpSetupCommands,
  ServerInfo,
  SidecarStatus,
  SlotBoardItem,
  UsageEvent,
} from './lib/api'
import { StatusDot } from './components/ui'
import { SlotsView } from './components/SlotsView'
import { BackendsView } from './components/BackendsView'
import { AccountsView } from './components/AccountsView'
import { ActivityView } from './components/ActivityView'
import { SettingsView } from './components/SettingsView'
import './App.css'

type Tab = 'slots' | 'backends' | 'accounts' | 'activity' | 'settings'

const TABS: { id: Tab; label: string; icon: string }[] = [
  { id: 'slots', label: 'Slots', icon: '◧' },
  { id: 'backends', label: 'Backends', icon: '▤' },
  { id: 'accounts', label: 'Accounts', icon: '⚿' },
  { id: 'activity', label: 'Activity', icon: '≋' },
  { id: 'settings', label: 'Settings', icon: '⚙' },
]

type Toast = { id: number; msg: string; isError: boolean }

function App() {
  const [tab, setTab] = useState<Tab>('slots')
  const [slots, setSlots] = useState<SlotBoardItem[]>([])
  const [profiles, setProfiles] = useState<BackendProfileView[]>([])
  const [server, setServer] = useState<ServerInfo | null>(null)
  const [commands, setCommands] = useState<McpSetupCommands | null>(null)
  const [sidecar, setSidecar] = useState<SidecarStatus | null>(null)
  const [accountsList, setAccountsList] = useState<AccountsList | null>(null)
  const [usage, setUsage] = useState<UsageEvent[]>([])
  const [autostart, setAutostart] = useState(false)
  const [online, setOnline] = useState(true)
  const [toasts, setToasts] = useState<Toast[]>([])
  const toastId = useRef(0)

  const notify = useCallback((msg: string, isError = false) => {
    const id = ++toastId.current
    setToasts((t) => [...t, { id, msg, isError }])
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), isError ? 7000 : 3500)
  }, [])

  const refresh = useCallback(async () => {
    try {
      const [board, pro, info, cmds, sc, acc, recent] = await Promise.all([
        api.slotBoard(),
        api.profiles(),
        api.serverInfo(),
        api.setupCommands(),
        api.sidecarStatus(),
        api.accounts(),
        api.recentUsage(60),
      ])
      setSlots(board)
      setProfiles(pro)
      setServer(info)
      setCommands(cmds)
      setSidecar(sc)
      setAccountsList(acc)
      setUsage(recent)
      setOnline(true)
    } catch {
      setOnline(false)
    }
  }, [])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 3000)
    isAutostartEnabled()
      .then(setAutostart)
      .catch(() => setAutostart(false))
    return () => clearInterval(t)
  }, [refresh])

  const workerOk = slots.some((s) => s.last_success !== false)

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">◎</span>
          <span className="brand-name">Orchestrator</span>
        </div>
        <nav>
          {TABS.map((t) => (
            <button
              key={t.id}
              className={`nav-item ${tab === t.id ? 'nav-active' : ''}`}
              onClick={() => setTab(t.id)}
            >
              <span className="nav-icon">{t.icon}</span>
              {t.label}
              {t.id === 'accounts' && sidecar?.enabled && (
                <StatusDot state={sidecar.healthy ? 'ok' : 'warn'} />
              )}
            </button>
          ))}
        </nav>
        <footer className="sidebar-foot">
          <div className="server-chip" title={server?.mcp_url ?? ''}>
            <StatusDot state={online ? (workerOk ? 'ok' : 'warn') : 'err'} pulse={!online} />
            <span className="mono">{online ? (server?.listen ?? '…') : 'offline'}</span>
          </div>
        </footer>
      </aside>

      <main className="content">
        {tab === 'slots' && <SlotsView slots={slots} profiles={profiles} notify={notify} refresh={refresh} />}
        {tab === 'backends' && <BackendsView profiles={profiles} notify={notify} refresh={refresh} />}
        {tab === 'accounts' && (
          <AccountsView sidecar={sidecar} accountsList={accountsList} notify={notify} refresh={refresh} />
        )}
        {tab === 'activity' && <ActivityView usage={usage} />}
        {tab === 'settings' && (
          <SettingsView
            server={server}
            commands={commands}
            autostart={autostart}
            setAutostart={setAutostart}
            notify={notify}
          />
        )}
      </main>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.isError ? 'toast-err' : ''}`}>
            {t.msg}
          </div>
        ))}
      </div>
    </div>
  )
}

export default App
