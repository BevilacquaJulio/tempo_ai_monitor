import { expect, it } from 'vitest'
import type { SessionRecord } from '@/providers/types'
import { codexAccountUsage, dailyUsage, filterSessions, heatmapData, modelBreakdown, projectBreakdown } from './activity-data'

const now = new Date(2026, 10, 13, 15).getTime()

function session(id: string, day: number, options: Partial<SessionRecord> = {}): SessionRecord {
  return {
    id, providerId: 'claude', projectName: 'Relay', model: 'Sonnet', startedAt: null,
    lastActivityAt: new Date(2026, 10, day, 12).toISOString(),
    inputTokens: null, outputTokens: null, cacheTokens: null, totalTokens: null,
    source: 'demo', updatedAt: new Date(now).toISOString(), ...options,
  }
}

it('combines all filters, uses calendar day boundaries and excludes future activity', () => {
  const sessions = [
    session('outside', 6), session('first-day', 7), session('today', 13), session('future', 14),
    session('provider', 13, { providerId: 'codex' }), session('model', 13, { model: 'Opus' }),
    session('project', 13, { projectName: 'Atlas' }),
  ]
  const filters = { period: 7, provider: 'claude', model: 'Sonnet', project: 'Relay' } as const
  expect(filterSessions(sessions, filters, now).map(({ id }) => id)).toEqual(['today', 'first-day'])
  expect(filterSessions(sessions, { ...filters, period: 30 }, now).map(({ id }) => id)).toEqual(['today', 'first-day', 'outside'])
})

it('sums known totals and distinguishes actual zero from unknown usage', () => {
  const sessions = [session('unknown', 11), session('zero', 12, { totalTokens: 0 }), session('a', 13, { totalTokens: 4 }), session('b', 13, { totalTokens: 6 })]
  expect(dailyUsage(sessions, 7, now).map(({ tokens }) => tokens)).toEqual([null, null, null, null, null, 0, 10])
})

it('keeps the Codex day labels without converting them as UTC timestamps', () => {
  const buckets = [{ day: '2026-11-12', tokens: 0 }, { day: '2026-11-13', tokens: 95 }]
  expect(codexAccountUsage(buckets, 7, now).slice(-3)).toEqual([
    { label: 'Nov 11', tokens: null }, { label: 'Nov 12', tokens: 0 }, { label: 'Nov 13', tokens: 95 },
  ])
})

it('filters by branch and builds heatmap, project and model breakdowns', () => {
  const sessions = [
    session('a', 13, { projectName: 'Alpha', gitBranch: 'main', model: 'claude-sonnet-4-5', inputTokens: 1000, outputTokens: 500, cacheReadTokens: 3000, cacheTokens: 3000, totalTokens: 4500 }),
    session('b', 12, { projectName: 'Beta', gitBranch: 'feat/ui', model: 'gpt-5.3-codex', inputTokens: 500, outputTokens: 200, cacheReadTokens: 300, cacheTokens: 300, totalTokens: 1000 }),
  ]
  expect(filterSessions(sessions, { period: 90, provider: '', model: '', project: '', branch: 'feat/ui' }, now).map(({ id }) => id)).toEqual(['b'])

  const heat = heatmapData(sessions, 30, now)
  expect(heat).toHaveLength(30)
  expect(heat[heat.length - 1].tokens).toBe(4500)
  expect(heat[heat.length - 1].level).toBe(4)
  expect(heat[heat.length - 2].tokens).toBe(1000)
  expect(heat[heat.length - 2].level).toBeGreaterThan(0)

  const projects = projectBreakdown(sessions)
  expect(projects[0].name).toBe('Alpha')
  expect(projects[0].tokens).toBe(4500)

  const models = modelBreakdown(sessions)
  expect(models.map((m) => m.name)).toEqual(['claude-sonnet-4-5', 'gpt-5.3-codex'])
})
