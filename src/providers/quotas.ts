import type { ProviderId, ProviderSnapshot, QuotaWindow } from './types'

export const thresholds = [80, 90, 95, 100] as const
export type UsageThresholds = readonly [number, number, number, number]

const antigravityLimits: Record<string, readonly [string, string]> = {
  '3p-5h': ['5-hour window', 'Claude and GPT models'],
  '3p-weekly': ['Weekly limit', 'Claude and GPT models'],
  'gemini-5h': ['5-hour window', 'Gemini models'],
  'gemini-weekly': ['Weekly limit', 'Gemini models'],
  'gemini-argon-wtu-weekly': ['Weekly limit', 'Gemini Argon'],
}

export function quotaLabel(quota: QuotaWindow, provider: ProviderId, t: (message: string) => string) {
  const known = (provider === 'antigravity' || provider === 'local-antigravity') && Object.hasOwn(antigravityLimits, quota.label)
  const [label, scope] = known ? antigravityLimits[quota.label] : [quota.label, quota.scope]
  return scope ? `${t(label)} · ${known ? t(scope) : scope}` : t(label)
}

export function usageLevel(used: number, [attention, elevated, critical, limit]: UsageThresholds = thresholds) {
  if (used >= limit) return { label: 'Limit reached', tone: 'danger' } as const
  if (used >= critical) return { label: 'About to run out', tone: 'danger' } as const
  if (used >= elevated) return { label: 'Very little left', tone: 'warning' } as const
  if (used >= attention) return { label: 'Getting close to the limit', tone: 'warning' } as const
  return { label: 'Room to keep using', tone: 'accent' } as const
}

export function isPercentage(value: number | null): value is number {
  return value !== null && Number.isFinite(value) && value >= 0 && value <= 100
}

export function trustedReset(quota: QuotaWindow): number | null {
  if (quota.status !== 'available' || !quota.resetAt || !quota.source || quota.source === 'local') return null
  const reset = Date.parse(quota.resetAt)
  return Number.isFinite(reset) ? reset : null
}

export function nextReset(providers: ProviderSnapshot[], now: number) {
  let next: { provider: ProviderSnapshot; quota: QuotaWindow; at: number } | null = null
  for (const provider of providers) {
    if (provider.integration !== 'connected' && provider.integration !== 'demo') continue
    for (const quota of provider.quotas) {
      const at = trustedReset(quota)
      if (at !== null && at > now && (next === null || at < next.at)) next = { provider, quota, at }
    }
  }
  return next
}

export function countdown(at: number, now: number) {
  const seconds = Math.max(0, Math.ceil((at - now) / 1000))
  if (!seconds) return 'Awaiting update'
  const minutes = Math.floor(seconds / 60)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)
  if (days) return `${days}d ${hours % 24}h`
  if (hours) return `${hours}h ${minutes % 60}m`
  return minutes ? `${minutes}m ${seconds % 60}s` : `${seconds}s`
}

export function isResetDue(quota: QuotaWindow, now: number) {
  const at = quota.resetAt ? Date.parse(quota.resetAt) : NaN
  return at <= now
}

export function currentUsage(quota: QuotaWindow, now: number): number | null {
  return quota.status === 'available' && !isResetDue(quota, now) && isPercentage(quota.usedPercent)
    ? quota.usedPercent : null
}
