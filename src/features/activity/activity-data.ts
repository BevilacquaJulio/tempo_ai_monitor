import type { Language } from '@/lib/locale'
import { formatDay } from '@/lib/format'
import { estimateSessionCost, summarizeCache } from '@/providers/pricing'
import type { ProviderId, SessionRecord } from '@/providers/types'

export type Period = 7 | 30 | 90
export interface SessionFilters {
  period: Period
  provider: ProviderId | ''
  model: string
  project: string
  branch?: string
}

function dayStart(now: number, offset: number) {
  const date = new Date(now)
  date.setDate(date.getDate() + offset)
  date.setHours(0, 0, 0, 0)
  return date.getTime()
}

function dayKey(epoch: number) {
  const local = new Date(epoch)
  return `${local.getFullYear()}-${String(local.getMonth() + 1).padStart(2, '0')}-${String(local.getDate()).padStart(2, '0')}`
}

export function filterSessions(sessions: SessionRecord[], filters: SessionFilters, now: number) {
  const start = dayStart(now, 1 - filters.period)
  return sessions.filter((session) => {
    const last = Date.parse(session.lastActivityAt)
    return last >= start && last <= now &&
      (!filters.provider || session.providerId === filters.provider) &&
      (!filters.model || session.model === filters.model) &&
      (!filters.project || session.projectName === filters.project) &&
      (!filters.branch || session.gitBranch === filters.branch)
  }).sort((a, b) => Date.parse(b.lastActivityAt) - Date.parse(a.lastActivityAt))
}

export function dailyUsage(sessions: SessionRecord[], period: Period, now: number, language: Language = 'en-US') {
  const totals = new Map<number, number>()
  for (const session of sessions) {
    if (session.totalTokens === null) continue
    const day = dayStart(Date.parse(session.lastActivityAt), 0)
    totals.set(day, (totals.get(day) ?? 0) + session.totalTokens)
  }
  return Array.from({ length: period }, (_, index) => {
    const day = dayStart(now, index + 1 - period)
    return { label: formatDay(day, language), tokens: totals.get(day) ?? null }
  })
}

export function codexAccountUsage(buckets: { day: string; tokens: number }[], period: Period, now: number, language: Language = 'en-US') {
  const totals = new Map(buckets.map(({ day, tokens }) => [day, tokens]))
  return Array.from({ length: period }, (_, index) => {
    const epoch = dayStart(now, index + 1 - period)
    const key = dayKey(epoch)
    return { label: formatDay(epoch, language), tokens: totals.get(key) ?? null }
  })
}

export interface HeatmapCell {
  key: string
  epoch: number
  label: string
  weekday: number
  tokens: number
  sessions: number
  costUsd: number | null
  level: 0 | 1 | 2 | 3 | 4
}

export function heatmapData(sessions: SessionRecord[], days: 30 | 90, now: number, language: Language = 'en-US'): HeatmapCell[] {
  const byDay = new Map<number, { tokens: number; sessions: number; costUsd: number; hasCost: boolean }>()
  const start = dayStart(now, 1 - days)
  for (const session of sessions) {
    const last = Date.parse(session.lastActivityAt)
    if (last < start || last > now) continue
    const day = dayStart(last, 0)
    const current = byDay.get(day) ?? { tokens: 0, sessions: 0, costUsd: 0, hasCost: false }
    current.sessions += 1
    if (session.totalTokens !== null) current.tokens += session.totalTokens
    const cost = estimateSessionCost(session)
    if (cost !== null) {
      current.costUsd += cost
      current.hasCost = true
    }
    byDay.set(day, current)
  }
  let maxScore = 0
  for (const bucket of byDay.values()) {
    const score = bucket.tokens > 0 ? bucket.tokens : bucket.sessions
    if (score > maxScore) maxScore = score
  }
  return Array.from({ length: days }, (_, index) => {
    const epoch = dayStart(now, index + 1 - days)
    const bucket = byDay.get(epoch)
    const tokens = bucket?.tokens ?? 0
    const count = bucket?.sessions ?? 0
    const score = tokens > 0 ? tokens : count
    const ratio = maxScore > 0 ? score / maxScore : 0
    const level: HeatmapCell['level'] = score === 0 ? 0 : ratio >= 0.75 ? 4 : ratio >= 0.45 ? 3 : ratio >= 0.18 ? 2 : 1
    return {
      key: dayKey(epoch),
      epoch,
      label: formatDay(epoch, language),
      weekday: new Date(epoch).getDay(),
      tokens,
      sessions: count,
      costUsd: bucket?.hasCost ? bucket.costUsd : null,
      level,
    }
  })
}

export interface BreakdownRow {
  name: string
  sessions: number
  tokens: number | null
  share: number
  costUsd: number | null
  cacheHitRate: number | null
}

function groupBreakdown(sessions: SessionRecord[], pick: (session: SessionRecord) => string): BreakdownRow[] {
  const groups = new Map<string, SessionRecord[]>()
  for (const session of sessions) {
    const name = pick(session)
    const list = groups.get(name)
    if (list) list.push(session)
    else groups.set(name, [session])
  }
  const rows = [...groups.entries()].map(([name, list]) => {
    let tokenSum = 0
    let anyTokens = false
    let costSum = 0
    let anyCost = false
    for (const item of list) {
      if (item.totalTokens !== null) {
        tokenSum += item.totalTokens
        anyTokens = true
      }
      const cost = estimateSessionCost(item)
      if (cost !== null) {
        costSum += cost
        anyCost = true
      }
    }
    const cache = summarizeCache(list)
    return {
      name,
      sessions: list.length,
      tokens: anyTokens ? tokenSum : null,
      costUsd: anyCost ? costSum : null,
      cacheHitRate: cache.hitRate,
    }
  }).sort((a, b) => (b.tokens ?? -1) - (a.tokens ?? -1) || b.sessions - a.sessions || a.name.localeCompare(b.name))

  const totalMetric = rows.reduce((sum, row) => sum + (row.tokens ?? row.sessions), 0)
  return rows.map((row) => ({
    ...row,
    share: totalMetric > 0 ? ((row.tokens ?? row.sessions) / totalMetric) * 100 : 0,
  }))
}

export function projectBreakdown(sessions: SessionRecord[], fallbackName = 'Unknown project'): BreakdownRow[] {
  return groupBreakdown(sessions, (session) => session.projectName ?? fallbackName)
}

export function modelBreakdown(sessions: SessionRecord[], fallbackName = 'Model unavailable'): BreakdownRow[] {
  return groupBreakdown(sessions, (session) => session.model ?? fallbackName)
}
