export type AgentId = 'claude' | 'codex' | 'antigravity' | 'gemini' | 'copilot' | 'cursor' | 'opencode' | 'windsurf' | 'aider' | 'amp'
export type ProviderId = AgentId | `local-${string}`
export type DataStatus = 'available' | 'unavailable' | 'stale' | 'error'
export type DataSource = 'official' | 'cli' | 'local' | 'demo'
export type QuotaWindowType = 'five_hour' | 'weekly' | 'custom'
export type IntegrationStatus = 'demo' | 'not_connected' | 'connected' | 'stale' | 'error'
export type ProviderWaitReason = 'credentials' | 'retry'

export interface QuotaWindow {
  id: string
  type: QuotaWindowType
  label: string
  scope: string | null
  usedPercent: number | null
  remainingPercent: number | null
  resetAt: string | null
  status: DataStatus
  source: DataSource | null
  updatedAt: string | null
}

export interface ProviderSnapshot {
  providerId: ProviderId
  displayName: string
  planName: string | null
  integration: IntegrationStatus
  connectionSaved?: boolean
  integrationError?: string | null
  checking?: boolean
  waitingFor?: ProviderWaitReason | null
  nextRetryAt?: string | null
  lastCheckedAt?: string | null
  quotas: QuotaWindow[]
  updatedAt: string | null
}

export interface SessionRecord {
  id: string
  providerId: ProviderId
  projectName: string | null
  gitBranch?: string | null
  model: string | null
  startedAt: string | null
  lastActivityAt: string
  inputTokens: number | null
  outputTokens: number | null
  cacheReadTokens?: number | null
  cacheCreationTokens?: number | null
  cacheTokens: number | null
  totalTokens: number | null
  requestCount?: number | null
  source: DataSource
  updatedAt: string
}

export interface UsageSnapshot {
  mode: 'demo' | 'live'
  providers: ProviderSnapshot[]
  sessions: SessionRecord[]
  observedAt: string | null
  historyError?: boolean
  codexDailyTokens?: { day: string; tokens: number }[] | null
  codexDailyUpdatedAt?: string | null
}

export type UsageReader = () => Promise<UsageSnapshot>
