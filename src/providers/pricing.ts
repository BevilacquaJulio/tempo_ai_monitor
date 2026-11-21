import type { SessionRecord } from './types'

/** Public API reference rates in USD per 1M tokens. Used strictly for local API-equivalent estimates, never subscription billing. */
interface ModelRate {
  input: number
  output: number
  cacheRead: number
  cacheWrite: number
}

const RATES: readonly [RegExp, ModelRate][] = [
  [/claude[- ]opus[- ]4[.-][56]/i, { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 }],
  [/claude[- ]opus/i, { input: 15, output: 75, cacheRead: 1.5, cacheWrite: 18.75 }],
  [/claude[- ]sonnet/i, { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75 }],
  [/claude[- ]haiku[- ]4/i, { input: 1, output: 5, cacheRead: 0.1, cacheWrite: 1.25 }],
  [/claude[- ]3[.-]5[- ]haiku|claude[- ]haiku/i, { input: 0.8, output: 4, cacheRead: 0.08, cacheWrite: 1 }],
  [/codex[- ]mini|gpt-5[.-]?mini/i, { input: 0.25, output: 2, cacheRead: 0.025, cacheWrite: 0.25 }],
  [/gpt-5|gpt[- ]codex|codex/i, { input: 1.25, output: 10, cacheRead: 0.125, cacheWrite: 1.25 }],
  [/gpt-4\.1[- ]mini/i, { input: 0.4, output: 1.6, cacheRead: 0.1, cacheWrite: 0.4 }],
  [/gpt-4\.1/i, { input: 2, output: 8, cacheRead: 0.5, cacheWrite: 2 }],
  [/gpt-4o[- ]mini/i, { input: 0.15, output: 0.6, cacheRead: 0.075, cacheWrite: 0.15 }],
  [/gpt-4o/i, { input: 2.5, output: 10, cacheRead: 1.25, cacheWrite: 2.5 }],
  [/\bo4-mini\b/i, { input: 1.1, output: 4.4, cacheRead: 0.275, cacheWrite: 1.1 }],
  [/\bo3-mini\b/i, { input: 1.1, output: 4.4, cacheRead: 0.55, cacheWrite: 1.1 }],
  [/\bo3\b/i, { input: 2, output: 8, cacheRead: 0.5, cacheWrite: 2 }],
]

export function lookupRate(model: string | null): ModelRate | null {
  if (!model) return null
  const match = RATES.find(([pattern]) => pattern.test(model))
  return match ? match[1] : null
}

function cacheParts(session: SessionRecord): { read: number; write: number } {
  if (typeof session.cacheReadTokens === 'number' || typeof session.cacheCreationTokens === 'number') {
    return {
      read: Math.max(0, session.cacheReadTokens ?? 0),
      write: Math.max(0, session.cacheCreationTokens ?? 0),
    }
  }
  return { read: Math.max(0, session.cacheTokens ?? 0), write: 0 }
}

export function estimateSessionCost(session: SessionRecord): number | null {
  const rate = lookupRate(session.model)
  if (!rate || session.totalTokens === null) return null
  const input = Math.max(0, session.inputTokens ?? 0)
  const output = Math.max(0, session.outputTokens ?? 0)
  const { read, write } = cacheParts(session)
  if (input + output + read + write === 0) return 0
  return (input * rate.input + output * rate.output + read * rate.cacheRead + write * rate.cacheWrite) / 1_000_000
}

export function estimateSessionCacheSavings(session: SessionRecord): number | null {
  const rate = lookupRate(session.model)
  if (!rate || session.totalTokens === null) return null
  const { read } = cacheParts(session)
  if (read === 0) return 0
  return (read * Math.max(0, rate.input - rate.cacheRead)) / 1_000_000
}

export interface CacheSummary {
  hitRate: number | null
  cacheRead: number
  cacheCreation: number
  savedUsd: number | null
}

export function summarizeCache(sessions: SessionRecord[]): CacheSummary {
  let promptDenominator = 0
  let cacheRead = 0
  let cacheCreation = 0
  let savedUsd = 0
  let anySaved = false
  for (const session of sessions) {
    if (session.totalTokens === null) continue
    const input = Math.max(0, session.inputTokens ?? 0)
    const { read, write } = cacheParts(session)
    promptDenominator += input + read + write
    cacheRead += read
    cacheCreation += write
    const saved = estimateSessionCacheSavings(session)
    if (saved !== null) {
      savedUsd += saved
      anySaved = true
    }
  }
  return {
    hitRate: promptDenominator > 0 ? (cacheRead / promptDenominator) * 100 : null,
    cacheRead,
    cacheCreation,
    savedUsd: anySaved ? savedUsd : null,
  }
}

export function summarizeCost(sessions: SessionRecord[]): number | null {
  let total = 0
  let known = false
  for (const session of sessions) {
    const cost = estimateSessionCost(session)
    if (cost !== null) {
      total += cost
      known = true
    }
  }
  return known ? total : null
}
