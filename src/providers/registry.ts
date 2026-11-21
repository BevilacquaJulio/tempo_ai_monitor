import type { AgentId, ProviderId, ProviderSnapshot, QuotaWindow } from './types'

export const providerNames: Record<AgentId, string> = {
  claude: 'Claude Code',
  codex: 'OpenAI Codex',
  antigravity: 'Google Antigravity',
  gemini: 'Gemini CLI',
  copilot: 'GitHub Copilot CLI',
  cursor: 'Cursor',
  opencode: 'OpenCode',
  windsurf: 'Windsurf',
  aider: 'Aider',
  amp: 'Amp',
}

export const providerName = (id: ProviderId) => providerNames[id as AgentId] ?? id

const windows = [
  { type: 'five_hour', label: '5-hour window' },
  { type: 'weekly', label: 'Weekly limit' },
] as const

export function unconnectedProviders(): ProviderSnapshot[] {
  return (['claude', 'codex'] as const).map((providerId) => ({
    providerId,
    displayName: providerNames[providerId],
    planName: null,
    integration: 'not_connected',
    updatedAt: null,
    quotas: windows.map((window): QuotaWindow => ({
      ...window,
      id: `${providerId}-${window.type}`,
      scope: null,
      usedPercent: null,
      remainingPercent: null,
      resetAt: null,
      status: 'unavailable',
      source: null,
      updatedAt: null,
    })),
  }))
}
