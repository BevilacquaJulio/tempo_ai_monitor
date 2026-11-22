import { formatCompact, formatNumber, formatPercent, formatUsd } from '@/lib/format'
import type { Language } from '@/lib/locale'
import { summarizeCache, summarizeCost } from '@/providers/pricing'
import { providerName } from '@/providers/registry'
import type { ProviderId, SessionRecord } from '@/providers/types'
import { modelBreakdown, projectBreakdown, type Period } from './activity-data'

export interface ShareOptions {
  period: Period
  provider: ProviderId | ''
  theme: 'dark' | 'light'
  hidePrivateNames: boolean
  language: Language
  t: (message: string, params?: Record<string, string | number>) => string
}

function escapeXml(text: string) {
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;')
}

/** Escapes Markdown formatting and raw HTML delimiters so metadata always renders as literal text. */
function escapeMarkdown(text: string) {
  return text
    .replace(/[\r\n\t]+/g, ' ')
    .replaceAll('\\', '\\\\')
    .replace(/([`*_{}[\]()#!<>|~])/g, '\\$1')
    .replace(/^([+-])/, '\\$1')
}

export function buildShareStats(sessions: SessionRecord[], options: ShareOptions) {
  const tokens = sessions.reduce((sum, s) => sum + (s.totalTokens ?? 0), 0)
  const hasTokens = sessions.some((s) => s.totalTokens !== null)
  const cost = summarizeCost(sessions)
  const cache = summarizeCache(sessions)
  const models = modelBreakdown(sessions).slice(0, 3)
  const projects = options.hidePrivateNames ? [] : projectBreakdown(sessions).slice(0, 3)
  const providerLabel = options.provider ? providerName(options.provider) : options.t('All AI tools')
  return {
    sessionsCount: sessions.length,
    tokens: hasTokens ? tokens : null,
    cost,
    cache,
    models,
    projects,
    providerLabel,
  }
}

export function buildShareSvg(sessions: SessionRecord[], options: ShareOptions): string {
  const { language, t, period, theme, hidePrivateNames } = options
  const stats = buildShareStats(sessions, options)
  const dark = theme === 'dark'
  const bg = dark ? '#060919' : '#f3f5fc'
  const surface = dark ? '#0c1228' : '#ffffff'
  const border = dark ? '#202d59' : '#dce2f2'
  const fg = dark ? '#eef2ff' : '#0b112b'
  const muted = dark ? '#96a4cc' : '#505c7d'
  const accent = dark ? '#6c88ff' : '#3b55f5'
  const accentSoft = dark ? '#172145' : '#e8edff'
  const success = dark ? '#4cc38a' : '#16805a'

  const tokenText = stats.tokens !== null ? formatCompact(stats.tokens, language) : t('Unavailable')
  const costText = formatUsd(stats.cost, language)
  const cacheText = stats.cache.hitRate !== null ? `${formatPercent(stats.cache.hitRate, language)}%` : t('Unavailable')
  const title = escapeXml(`TempoAI · ${stats.providerLabel}`)
  const subtitle = escapeXml(t('Last {count} days · Local usage snapshot', { count: period }))
  const privacyBadge = escapeXml(hidePrivateNames ? t('Privacy mode · Project names hidden') : t('Includes top project names'))

  const modelRows = stats.models.length > 0
    ? stats.models.map((row, index) => {
      const y = 212 + index * 28
      const barWidth = Math.max(12, Math.round((row.share / 100) * 210))
      const label = escapeXml(row.name.length > 26 ? `${row.name.slice(0, 25)}…` : row.name)
      const val = escapeXml(row.tokens !== null ? `${formatCompact(row.tokens, language)} tokens` : `${row.sessions}`)
      return `
        <text x="36" y="${y}" fill="${fg}" font-size="12" font-weight="500">${label}</text>
        <rect x="236" y="${y - 10}" width="210" height="8" rx="4" fill="${border}" />
        <rect x="236" y="${y - 10}" width="${barWidth}" height="8" rx="4" fill="${accent}" />
        <text x="564" y="${y}" text-anchor="end" fill="${muted}" font-size="12">${val}</text>`
    }).join('')
    : `<text x="36" y="220" fill="${muted}" font-size="12">${escapeXml(t('No model breakdown in this period.'))}</text>`

  const projectLine = !hidePrivateNames && stats.projects.length > 0
    ? `<text x="36" y="314" fill="${muted}" font-size="11">${escapeXml(
      `${t('Top projects')}: ${stats.projects.map((p) => p.name).join(', ')}`,
    )}</text>`
    : ''

  return [
    `<svg xmlns="http://www.w3.org/2000/svg" width="600" height="352" viewBox="0 0 600 352" role="img" aria-label="${title}">`,
    `  <rect width="600" height="352" rx="18" fill="${bg}" />`,
    `  <rect x="1" y="1" width="598" height="350" rx="17" fill="none" stroke="${border}" />`,
    `  <path d="M24 20h12l-5 7 5 7H24l5-7z" fill="${accent}" />`,
    `  <text x="44" y="32" fill="${fg}" font-size="16" font-weight="650" font-family="Segoe UI, system-ui, sans-serif">${title}</text>`,
    `  <text x="24" y="54" fill="${muted}" font-size="12" font-family="Segoe UI, system-ui, sans-serif">${subtitle}</text>`,
    `  <rect x="378" y="20" width="198" height="24" rx="12" fill="${accentSoft}" />`,
    `  <text x="477" y="36" text-anchor="middle" fill="${accent}" font-size="10.5" font-weight="600" font-family="Segoe UI, system-ui, sans-serif">${privacyBadge}</text>`,
    `  <g font-family="Segoe UI, system-ui, sans-serif">`,
    `    <rect x="24" y="74" width="132" height="72" rx="10" fill="${surface}" stroke="${border}" />`,
    `    <text x="36" y="96" fill="${muted}" font-size="11">${escapeXml(t('Sessions'))}</text>`,
    `    <text x="36" y="128" fill="${fg}" font-size="22" font-weight="650">${escapeXml(formatNumber(stats.sessionsCount, language))}</text>`,
    `    <rect x="164" y="74" width="132" height="72" rx="10" fill="${surface}" stroke="${border}" />`,
    `    <text x="176" y="96" fill="${muted}" font-size="11">${escapeXml(t('Total tokens'))}</text>`,
    `    <text x="176" y="128" fill="${fg}" font-size="22" font-weight="650">${escapeXml(tokenText)}</text>`,
    `    <rect x="304" y="74" width="132" height="72" rx="10" fill="${surface}" stroke="${border}" />`,
    `    <text x="316" y="96" fill="${muted}" font-size="11">${escapeXml(t('Cache hit rate'))}</text>`,
    `    <text x="316" y="128" fill="${success}" font-size="22" font-weight="650">${escapeXml(cacheText)}</text>`,
    `    <rect x="444" y="74" width="132" height="72" rx="10" fill="${surface}" stroke="${border}" />`,
    `    <text x="456" y="96" fill="${muted}" font-size="11">${escapeXml(t('API-eq. est.'))}</text>`,
    `    <text x="456" y="128" fill="${fg}" font-size="22" font-weight="650">${escapeXml(costText)}</text>`,
    `  </g>`,
    `  <g font-family="Segoe UI, system-ui, sans-serif">`,
    `    <rect x="24" y="162" width="552" height="132" rx="12" fill="${surface}" stroke="${border}" />`,
    `    <text x="36" y="186" fill="${muted}" font-size="11" font-weight="600">${escapeXml(t('Top models'))}</text>`,
    modelRows,
    `  </g>`,
    `  <g font-family="Segoe UI, system-ui, sans-serif">`,
    projectLine,
    `    <text x="24" y="336" fill="${muted}" font-size="10.5">${escapeXml(t('Generated locally by TempoAI · API-equivalent cost is an estimate, separate from subscription billing.'))}</text>`,
    `  </g>`,
    `</svg>`,
  ].join('\n')
}

export function buildShareMarkdown(sessions: SessionRecord[], options: ShareOptions): string {
  const { language, t, period, hidePrivateNames } = options
  const stats = buildShareStats(sessions, options)
  const lines = [
    `### TempoAI — ${escapeMarkdown(stats.providerLabel)} (${t('{count} days', { count: period })})`,
    '',
    `- **${t('Sessions')}**: ${formatNumber(stats.sessionsCount, language)}`,
    `- **${t('Total tokens')}**: ${stats.tokens !== null ? formatNumber(stats.tokens, language) : t('Unavailable')}`,
    `- **${t('Prompt cache efficiency')}**: ${stats.cache.hitRate !== null ? `${formatPercent(stats.cache.hitRate, language)}%` : t('Unavailable')}`,
    `- **${t('API-equivalent cost (est.)')}**: ${formatUsd(stats.cost, language)}`,
  ]
  if (stats.models.length > 0) {
    lines.push('', `**${t('Top models')}**`)
    for (const row of stats.models) {
      const tokens = row.tokens !== null ? `${formatCompact(row.tokens, language)} tokens` : `${row.sessions}`
      const cost = row.costUsd !== null ? ` (${formatUsd(row.costUsd, language)})` : ''
      lines.push(`- ${escapeMarkdown(row.name)}: ${tokens}${cost}`)
    }
  }
  if (!hidePrivateNames && stats.projects.length > 0) {
    lines.push('', `**${t('Top projects')}**: ${stats.projects.map((p) => escapeMarkdown(p.name)).join(', ')}`)
  }
  lines.push('', `_${t('Generated locally by TempoAI. Cost is an API-equivalent estimate, separate from subscription billing.')}_`)
  return lines.join('\n')
}
