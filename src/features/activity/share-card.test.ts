import { expect, it } from 'vitest'
import type { SessionRecord } from '@/providers/types'
import { buildShareMarkdown, buildShareSvg } from './share-card'

const identity = (message: string, params?: Record<string, string | number>) =>
  params ? message.replace(/\{(\w+)\}/g, (match, key: string) => String(params[key] ?? match)) : message

const sample: SessionRecord[] = [
  {
    id: 'claude:1',
    providerId: 'claude',
    projectName: 'Secret <Repo> & Co',
    gitBranch: 'feat/private',
    model: 'claude-sonnet-4-5',
    startedAt: '2026-11-20T10:00:00Z',
    lastActivityAt: '2026-11-20T11:00:00Z',
    inputTokens: 10_000,
    outputTokens: 5_000,
    cacheReadTokens: 40_000,
    cacheCreationTokens: 5_000,
    cacheTokens: 45_000,
    totalTokens: 60_000,
    requestCount: 6,
    source: 'local',
    updatedAt: '2026-11-20T11:00:00Z',
  },
]

it('builds deterministic SVG and Markdown while hiding private project names when privacy mode is active', () => {
  const privateSvg = buildShareSvg(sample, {
    period: 30,
    provider: '',
    theme: 'dark',
    hidePrivateNames: true,
    language: 'en-US',
    t: identity,
  })
  expect(privateSvg).toContain('<svg ')
  expect(privateSvg).toContain('claude-sonnet-4-5')
  expect(privateSvg).not.toContain('Secret')

  const publicSvg = buildShareSvg(sample, {
    period: 30,
    provider: 'claude',
    theme: 'light',
    hidePrivateNames: false,
    language: 'en-US',
    t: identity,
  })
  expect(publicSvg).toContain('Secret &lt;Repo&gt; &amp; Co')

  const md = buildShareMarkdown(sample, {
    period: 30,
    provider: '',
    theme: 'dark',
    hidePrivateNames: true,
    language: 'en-US',
    t: identity,
  })
  expect(md).toContain('TempoAI')
  expect(md).toContain('claude-sonnet-4-5')
  expect(md).not.toContain('Secret')
})
