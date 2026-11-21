import { describe, expect, it } from 'vitest'
import { countdown, nextReset, usageLevel } from './quotas'
import { unconnectedProviders } from './registry'
import type { QuotaWindow } from './types'

describe('quota rules', () => {
  it.each([
    [0, 'Room to keep using'], [79, 'Room to keep using'], [80, 'Getting close to the limit'],
    [89, 'Getting close to the limit'], [90, 'Very little left'], [94, 'Very little left'],
    [95, 'About to run out'], [99, 'About to run out'], [100, 'Limit reached'],
  ])('classifies %i%% as %s', (percent, label) => {
    expect(usageLevel(percent).label).toBe(label)
  })

  it('keeps expired, local, stale and invalid reset times out of Next Reset', () => {
    const [claude, codex] = unconnectedProviders()
    claude.integration = codex.integration = 'connected'
    const now = Date.parse('2026-11-13T15:00:00Z')
    const base: QuotaWindow = { ...claude.quotas[0], status: 'available', source: 'official' }
    claude.quotas = [
      { ...base, resetAt: '2026-11-13T14:00:00Z' },
      { ...base, status: 'stale', resetAt: '2026-11-13T15:10:00Z' },
      { ...base, source: 'local', resetAt: '2026-11-13T15:05:00Z' },
      { ...base, resetAt: 'invalid' },
      { ...base, resetAt: '2026-11-13T16:00:00Z' },
    ]
    codex.quotas = [{ ...base, type: 'weekly', scope: 'Model A', resetAt: '2026-11-13T15:20:00Z' }]
    expect(nextReset([claude, codex], now)).toMatchObject({
      provider: { providerId: 'codex' }, quota: { type: 'weekly', scope: 'Model A' },
    })
  })

  it('calculates from the clock and waits for confirmation after reaching zero', () => {
    expect(countdown(90_000, 0)).toBe('1m 30s')
    expect(countdown(90_000, 89_999)).toBe('1s')
    expect(countdown(90_000, 90_000)).toBe('Awaiting update')
    expect(countdown(90_000, 300_000)).toBe('Awaiting update')
  })
})
