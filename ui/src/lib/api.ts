// Typed wrappers around the Tauri command surface.
import { invoke } from '@tauri-apps/api/core'

export type SlotBoardItem = {
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

export type BackendProfileView = {
  id: string
  label: string
  backend: string
  base_url: string
  model: string
  auth_ref: string | null
}

export type ServerInfo = {
  listen: string
  mcp_url: string
  health_url: string
  config_path: string
}

export type McpSetupCommands = { claude: string; codex: string }

export type SidecarStatus = {
  enabled: boolean
  running: boolean
  healthy: boolean
  presence: string
  binary_present: boolean
  has_auth_credentials: boolean
  version_pin: string
  base_url: string
  openai_base_url: string
  port: number
  binary_path: string
  config_path: string
  last_error: string | null
  restart_count: number
}

export type AuthAccount = {
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

export type AccountsList = {
  state: 'ok' | 'disabled' | 'not_installed' | 'stopped' | 'unhealthy'
  message: string
  accounts: AuthAccount[]
}

export type OllamaModel = {
  name: string
  host: string
  openai_base_url: string
}

export type UsageEvent = {
  ts: string
  slot: string
  profile_id?: string | null
  base_url: string
  model: string
  latency_ms: number
  success: boolean
  reason?: string | null
}

export type UpdateCheck = {
  available: boolean
  current_version: string
  latest_version: string | null
  body: string | null
  message: string
}

export const api = {
  serverInfo: () => invoke<ServerInfo>('get_server_info'),
  slotBoard: () => invoke<SlotBoardItem[]>('get_slot_board'),
  openLogDir: () => invoke<string>('open_log_dir'),
  profiles: () => invoke<BackendProfileView[]>('get_backend_profiles'),
  setupCommands: () => invoke<McpSetupCommands>('get_mcp_setup_commands'),

  swapSlot: (slotName: string, profileId: string) =>
    invoke('swap_slot_backend', { slotName, profileId }),
  addSlot: (name: string, description: string, profileId: string) =>
    invoke('add_slot', { args: { name, description, profileId } }),
  removeSlot: (name: string) => invoke('remove_slot', { name }),
  updateSlotDescription: (name: string, description: string) =>
    invoke('update_slot_description', { name, description }),
  setSlotFallback: (name: string, enableFallback: boolean, fallback: string[]) =>
    invoke('set_slot_fallback', { args: { name, enableFallback, fallback } }),

  upsertProfile: (p: {
    id: string
    label: string
    backend: string
    baseUrl: string
    model: string
    authRef: string | null
  }) => invoke('upsert_backend_profile', { args: p }),
  removeProfile: (id: string) => invoke('remove_backend_profile', { id }),
  setSecret: (name: string, value: string) => invoke('set_secret', { name, value }),

  sidecarStatus: () => invoke<SidecarStatus>('get_sidecar_status'),
  setSidecarEnabled: (enabled: boolean) => invoke('set_sidecar_enabled', { enabled }),
  accounts: () => invoke<AccountsList>('list_subscription_accounts'),
  oauthProviders: () => invoke<string[]>('list_oauth_providers'),
  startOAuth: (provider: string) => invoke('start_subscription_oauth', { provider }),
  disconnectAccount: (name: string) => invoke('disconnect_subscription_account', { name }),
  syncProfiles: () => invoke<string[]>('sync_subscription_profiles'),
  proxyModels: () => invoke<string[]>('list_proxy_models'),
  setModelOverride: (accountId: string, model: string) =>
    invoke<string[]>('set_account_model_override', { accountId, model }),
  clearModelOverride: (accountId: string) =>
    invoke<string[]>('clear_account_model_override', { accountId }),

  ollamaHosts: () => invoke<string[]>('get_ollama_extra_hosts'),
  setOllamaHosts: (hosts: string[]) => invoke('set_ollama_extra_hosts', { hosts }),
  discoverOllama: () => invoke<OllamaModel[]>('discover_ollama_models'),
  createOllamaProfile: (host: string, model: string) =>
    invoke<string>('create_ollama_profile', { host, model }),

  recentUsage: (limit = 60) => invoke<UsageEvent[]>('get_recent_usage', { limit }),

  checkUpdates: () => invoke<UpdateCheck>('check_for_updates'),
  installUpdate: () => invoke<string>('install_update'),
}
