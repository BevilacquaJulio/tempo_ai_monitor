import type { DetectedAgent, Preferences, ProviderConnection } from '@/services/desktop'

export function zeroConfigConnection(agent: DetectedAgent): ProviderConnection | null {
  if (agent.id === 'claude') return { kind: 'claudeAuto' }
  if (agent.id === 'codex') return { kind: 'codexAuto' }
  if (agent.id === 'antigravity' && agent.sourcePath) return { kind: 'antigravityAuto' }
  return null
}

export function isNativeZeroConfig(agent: DetectedAgent): boolean {
  return zeroConfigConnection(agent) !== null
}

export function isAgentEnabled(agent: DetectedAgent, settings?: Preferences): boolean {
  if (!settings) return false
  if (agent.id === 'claude') return settings.claudeEnabled
  if (agent.id === 'codex') return settings.codexEnabled
  if (agent.id === 'antigravity') return settings.antigravityEnabled
  return false
}

export function agentSignals(agent: DetectedAgent): string[] {
  const tags: string[] = []
  if (isNativeZeroConfig(agent)) tags.push('Zero-config')
  if (agent.credentialsFound) tags.push('Sign-in found')
  if (agent.logsFound) tags.push('Local history found')
  if (!agent.credentialsFound && !agent.logsFound && agent.executable) tags.push('Installation detected')
  return tags
}

export function nativeConnectionDescription(agent: DetectedAgent): string {
  if (agent.id === 'claude') {
    return 'Connect with one click — zero configuration. TempoAI reads your local Claude Code sign-in and session token logs without editing settings or sending messages.'
  }
  if (agent.id === 'codex') {
    return 'Connect with one click — zero configuration. TempoAI reads your local Codex sign-in and session token logs automatically.'
  }
  return 'Use your Antigravity CLI sign-in to check usage in TempoAI without sending messages.'
}
