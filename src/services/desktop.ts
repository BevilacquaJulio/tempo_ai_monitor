import { invoke, isTauri } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import type { AgentId, ProviderId, SessionRecord, UsageSnapshot } from '@/providers/types'

export interface LocalProvider {
  id: `local-${string}`
  name: string
  path: string
}

export interface DetectedAgent {
  id: AgentId
  executable: string | null
  sourcePath: string | null
  credentialsFound?: boolean
  logsFound?: boolean
}

export interface ProviderScan {
  agents: DetectedAgent[]
}

export type ProviderConnection =
  | { kind: 'claudeAuto' }
  | { kind: 'codexAuto'; executable?: string | null }
  | { kind: 'antigravityAuto'; executable?: string | null }
  | { kind: 'local'; source: LocalProvider }

export interface Preferences {
  autostart: boolean
  minimizeToTray: boolean
  openPanelAtStartup: boolean
  pinPanel: boolean
  shortcut: string
  claudeEnabled: boolean
  codexEnabled: boolean
  codexExecutable?: string | null
  antigravityEnabled: boolean
  antigravityExecutable?: string | null
  notifications: boolean
  claudeAlerts: boolean
  codexAlerts: boolean
  antigravityAlerts?: boolean
  thresholds: number[]
  localProviders?: LocalProvider[]
}

export interface PreferencesState {
  settings: Preferences
  shortcutConflict: boolean
  recoveryWarning: string | null
}

export interface QuickBounds {
  x: number
  y: number
  width: number
  height: number
}

export interface UiState {
  language: 'en-US' | 'pt-BR' | null
  theme: 'system' | 'light' | 'dark'
  fontScale: number
  percentStyle: 'remaining' | 'used'
  perToolPercent: Record<string, 'remaining' | 'used'>
  collapsedOverview: string[]
  collapsedQuick: string[]
  lastRoute: '/' | '/activity' | '/settings'
  activityPeriod: 7 | 30 | 90
  heatmapDays: 30 | 90
  dismissedGuidance: string
  quickBounds?: QuickBounds | null
}

export interface UiStatePatch {
  language?: 'en-US' | 'pt-BR'
  theme?: 'system' | 'light' | 'dark'
  fontScale?: number
  percentStyle?: 'remaining' | 'used'
  perToolPercent?: Record<string, 'remaining' | 'used'>
  perToolOverride?: [string, 'remaining' | 'used' | null]
  collapsedOverview?: string[]
  collapsedQuick?: string[]
  lastRoute?: '/' | '/activity' | '/settings'
  activityPeriod?: 7 | 30 | 90
  heatmapDays?: 30 | 90
  dismissedGuidance?: string
}

export interface SessionHistory {
  sessions: SessionRecord[]
  historyError: boolean
}

export type PreferenceField = 'shortcut'

export interface PreferenceError {
  field?: PreferenceField
  message: string
}

export interface IntegrationCheck {
  providerId: ProviderId
  status: 'connected' | 'stale' | 'disabled' | 'error'
  message: string
}

export interface UpdateCheck {
  status: 'up_to_date' | 'update_available' | 'no_release'
  currentVersion: string
  latestVersion: string | null
  publishedAt: string | null
  notes: string | null
  releaseUrl: string
  installerUrl: string | null
  portableUrl: string | null
  runningPortable: boolean
}

export const desktop = isTauri()
export const quickWindow = desktop && getCurrentWebviewWindow().label === 'quick'
export const readDesktopUsage = () => invoke<UsageSnapshot>('usage_snapshot')
export const readDesktopSessions = () => invoke<SessionHistory>('session_history')
export const getPreferences = () => invoke<PreferencesState>('get_preferences')
export const getUiState = () => invoke<UiState>('get_ui_state')
export const saveUiState = (ui: UiStatePatch) => invoke<UiState>('save_ui_state', { ui })
export const scanProviders = () => invoke<ProviderScan>('scan_providers')
export const connectProvider = (connection: ProviderConnection) => invoke<PreferencesState>('connect_provider', { connection })
export const setDesktopLanguage = (portuguese: boolean) => invoke<void>('set_desktop_language', { portuguese })
export const savePreferences = (settings: Preferences) => invoke<PreferencesState>('save_preferences', { settings })
export const testSavedIntegrations = () => invoke<IntegrationCheck[]>('test_saved_integrations')
export const setPanelPin = (pinned: boolean) => invoke<void>('set_pin', { pinned })
export const refreshDesktop = () => invoke<void>('request_refresh')
export const refreshClaude = () => invoke<void>('refresh_claude')
export const refreshAntigravity = () => invoke<void>('refresh_antigravity')
export const getDataLocation = () => invoke<string>('data_location')
export const clearHistory = () => invoke<void>('clear_history')
export const openDashboard = (settings = false) => invoke<void>('open_dashboard', { settings })
export const hidePanel = () => invoke<void>('hide_panel')
export const removeProvider = (providerId: ProviderId) => invoke<PreferencesState>('remove_provider', { providerId })
export const checkForUpdates = () => invoke<UpdateCheck>('check_for_updates')
export const openReleaseUrl = (url: string) => invoke<void>('open_release_url', { url })
export const setWindowTheme = (theme: 'light' | 'dark' | null) => getCurrentWebviewWindow().setTheme(theme)
