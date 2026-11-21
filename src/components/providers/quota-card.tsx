import { ArrowLeftRight, CalendarDays, Timer } from 'lucide-react'
import { shownShare, useDisplay, type PercentStyle } from '@/lib/display'
import { formatDateTime, formatExpectedRenewal, formatPercent, formatRenewal } from '@/lib/format'
import { currentUsage, isPercentage, isResetDue, quotaLabel, trustedReset, usageLevel } from '@/providers/quotas'
import type { ProviderId, QuotaWindow } from '@/providers/types'
import { useLocale } from '@/lib/locale'

/** The share in words for one tool, following its "left" or "used" choice. */
export function useShareText(toolId: string) {
  const { t, language } = useLocale()
  const { styleFor } = useDisplay()
  return (used: number, style: PercentStyle = styleFor(toolId)) => t(style === 'remaining' ? '{value}% left' : '{value}% used',
    { value: formatPercent(shownShare(used, style), language) })
}

// Smooth, continuous sub-percent progress bar so values like 95%, 97% and 99% stay visually distinct.
export function UsageGauge({ used, label, tone, toolId }: { used: number; label: string; tone: string; toolId: string }) {
  const { styleFor } = useDisplay()
  const share = useShareText(toolId)
  const shown = Math.max(0, Math.min(100, shownShare(used, styleFor(toolId))))
  const critical = used >= 90
  return <div className={`gauge tone-${tone}${critical ? ' is-critical' : ''}`} role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100}
    aria-valuenow={used} aria-valuetext={share(used)}>
    <span className="gauge-fill" style={{ width: `${shown}%` }} />
  </div>
}

// Clicking the number flips only this tool between "left" and "used"; Settings holds the default for all tools.
export function ShareToggle({ used, toolId, className = 'quota-figure' }: { used: number; toolId: string; className?: string }) {
  const { t } = useLocale()
  const { styleFor, toggleStyleFor } = useDisplay()
  const style = styleFor(toolId)
  const text = useShareText(toolId)(used, style)
  const hint = t(style === 'remaining' ? 'Show how much was used' : 'Show how much is left')
  return <button type="button" className={className} title={hint} aria-label={`${text}. ${hint}`} onClick={() => toggleStyleFor(toolId)}>
    <span key={style} className="figure-value">{text}</span><ArrowLeftRight aria-hidden="true" />
  </button>
}

export function RenewalTime({ at, now }: { at: number; now: number }) {
  const { language } = useLocale()
  return <time className="countdown" dateTime={new Date(at).toISOString()} title={formatDateTime(at, language)}>
    {formatRenewal(at, now, language)}
  </time>
}

export function QuotaCard({ quota, provider, now }: { quota: QuotaWindow; provider: ProviderId; now: number }) {
  const { t, language } = useLocale()
  const weekly = quota.type === 'weekly' || (quota.type === 'custom' && /(?:^|[- _])weekly$/i.test(quota.label))
  const Icon = weekly ? CalendarDays : Timer
  const due = isResetDue(quota, now)
  const used = currentUsage(quota, now)
  const level = used === null ? null : usageLevel(used)
  const at = trustedReset(quota)
  const reportedReset = quota.resetAt ? Date.parse(quota.resetAt) : NaN
  const label = quotaLabel(quota, provider, t)
  const status = due ? 'Awaiting update' : quota.status === 'stale' ? 'Out of date' : quota.status === 'error' ? 'Update failed' : 'Unavailable'
  const lastReading = (quota.status === 'stale' || due) && isPercentage(quota.usedPercent)
    ? t('Last reading: {value}% used', { value: formatPercent(quota.usedPercent, language) }) : null
  return <section className="quota" aria-label={label}>
    <div className="quota-head">
      <span className="quota-name"><Icon aria-hidden="true" />{label}</span>
      {level && <span className={`quota-level tone-${level.tone}`}>{t(level.label)}</span>}
    </div>
    {used !== null && level ? <>
      <ShareToggle used={used} toolId={provider} />
      <UsageGauge used={used} label={t('{label} usage', { label })} tone={level.tone} toolId={provider} />
    </> : <p className="quota-unavailable">{t(status)}{lastReading && <span>{lastReading}</span>}</p>}
    <p className="quota-reset">
      {at !== null && !due ? (
        <time dateTime={quota.resetAt ?? undefined} title={formatDateTime(at, language)}>{formatRenewal(at, now, language)}</time>
      ) : (
        <span>{reportedReset > now
          ? formatExpectedRenewal(reportedReset, now, language)
          : t(Number.isFinite(reportedReset) || due ? 'Renewal unconfirmed' : 'Reset time unavailable')}</span>
      )}
    </p>
    {quota.status === 'stale' && quota.updatedAt && <p className="quota-reset">{t('Last observed {time}', { time: formatDateTime(quota.updatedAt, language) })}</p>}
  </section>
}
