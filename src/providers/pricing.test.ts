import { expect, it } from 'vitest'
import type { SessionRecord } from './types'
import { estimateSessionCost, lookupRate, summarizeCache, summarizeCost } from './pricing'

function makeSession(overrides: Partial<SessionRecord> = {}): SessionRecord {
  return {
    id: 's1',
    providerId: 'claude',
    projectName: 'tempo-ai',
    model: 'claude-sonnet-4-5-20250929',
    startedAt: null,
    lastActivityAt: '2026-11-20T12:00:00Z',
    inputTokens: 100_000,
    outputTokens: 50_000,
    cacheReadTokens: 800_000,
    cacheCreationTokens: 100_000,
    cacheTokens: 900_000,
    totalTokens: 1_050_000,
    source: 'local',
    updatedAt: '2026-11-20T12:00:00Z',
    ...overrides,
  }
}

it('estimates API-equivalent cost for Claude and Codex models and returns null for unknown models', () => {
  // Sonnet: 0.1M * $3 + 0.05M * $15 + 0.8M * $0.30 + 0.1M * $3.75 = 0.30 + 0.75 + 0.24 + 0.375 = $1.665
  expect(estimateSessionCost(makeSession())).toBeCloseTo(1.665, 4)

  // GPT-5.3 Codex: 0.2M * $1.25 + 0.1M * $10 + 0.8M * $0.125 = 0.25 + 1.00 + 0.10 = $1.35
  const codex = makeSession({
    providerId: 'codex',
    model: 'gpt-5.3-codex',
    inputTokens: 200_000,
    outputTokens: 100_000,
    cacheReadTokens: 800_000,
    cacheCreationTokens: 0,
    cacheTokens: 800_000,
    totalTokens: 1_100_000,
  })
  expect(estimateSessionCost(codex)).toBeCloseTo(1.35, 4)

  expect(lookupRate('unknown-local-model')).toBeNull()
  expect(estimateSessionCost(makeSession({ model: 'unknown-local-model' }))).toBeNull()
  expect(estimateSessionCost(makeSession({ totalTokens: null }))).toBeNull()
})

it('computes prompt cache hit rate and estimated USD savings', () => {
  const summary = summarizeCache([makeSession()])
  // cacheRead / (input + cacheRead + cacheCreation) = 800k / (100k + 800k + 100k) = 80%
  expect(summary.hitRate).toBeCloseTo(80, 2)
  expect(summary.cacheRead).toBe(800_000)
  expect(summary.cacheCreation).toBe(100_000)
  // Saved: 0.8M * ($3 - $0.30) = $2.16
  expect(summary.savedUsd).toBeCloseTo(2.16, 4)
  expect(summarizeCost([makeSession({ totalTokens: null })])).toBeNull()
})
