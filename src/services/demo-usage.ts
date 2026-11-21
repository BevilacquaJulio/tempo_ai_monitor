import { unconnectedProviders } from '@/providers/registry'
import type { ProviderSnapshot, QuotaWindow, SessionRecord, UsageReader } from '@/providers/types'

const minute = 60_000
const anchors = [
  { usage: [72, 43], remainingMinutes: [84, 3 * 1440 + 480], plan: 'Max' },
  { usage: [38, 86], remainingMinutes: [162, 2 * 1440 + 120], plan: 'Plus' },
]

function fixtureQuota(quota: QuotaWindow, percent: number, resetAt: string, updatedAt: string): QuotaWindow {
  return { ...quota, usedPercent: percent, remainingPercent: 100 - percent, resetAt, status: 'available', source: 'demo', updatedAt }
}

const demoEpoch = Date.now()

export const readDemoUsage: UsageReader = async () => {
  const now = Date.now()
  const observedAt = new Date(now).toISOString()
  const providers = unconnectedProviders().map((provider, index): ProviderSnapshot => {
    const fixture = anchors[index]
    const quotas = provider.quotas.map((quota, i) => fixtureQuota(
      quota, fixture.usage[i], new Date(demoEpoch + fixture.remainingMinutes[i] * minute).toISOString(), observedAt,
    ))
    return { ...provider, integration: 'demo', planName: fixture.plan, updatedAt: observedAt, quotas }
  })
  const projects = ['Atlas workspace', 'Relay CLI', 'Studio kit']
  const branches = ['main', 'feat/onboarding', 'fix/quotas', 'refactor/cache']
  const amounts = [36_100, 24_600, 51_800, 31_400, 68_200, 42_500, 28_300]
  const sessions: SessionRecord[] = Array.from({ length: 64 }, (_, index) => {
    const claude = index % 4 < 2
    const day = new Date(now)
    day.setDate(day.getDate() - index)
    day.setHours(11 + index % 5, (index * 13) % 60, 0, 0)
    const today = new Date(now).setHours(0, 0, 0, 0)
    const last = index === 0 ? Math.max(today, now - 25 * minute) : day.getTime()
    const total = Math.round(amounts[index % amounts.length] * (index > 28 ? 0.45 : index > 6 ? 0.65 : 1))
    const input = Math.round(total * 0.22)
    const output = Math.round(total * 0.18)
    const cacheTotal = total - input - output
    const cacheRead = Math.round(cacheTotal * (claude ? 0.82 : 1))
    const cacheCreation = cacheTotal - cacheRead
    return {
      id: `sample-${index}`, providerId: claude ? 'claude' : 'codex',
      projectName: projects[index % projects.length],
      gitBranch: branches[index % branches.length],
      model: claude ? (index % 3 === 0 ? 'Claude Opus' : 'Claude Sonnet') : 'GPT Codex',
      startedAt: new Date(last - (12 + index % 5 * 7) * minute).toISOString(),
      lastActivityAt: new Date(last).toISOString(),
      inputTokens: input, outputTokens: output,
      cacheReadTokens: cacheRead, cacheCreationTokens: cacheCreation, cacheTokens: cacheTotal,
      totalTokens: total, requestCount: 4 + (index % 11),
      source: 'demo', updatedAt: observedAt,
    }
  })
  return { mode: 'demo', providers, sessions, observedAt }
}
