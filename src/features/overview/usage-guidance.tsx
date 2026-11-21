import { CircleAlert, CircleCheck, Info, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useDisplay } from '@/lib/display'
import { formatPercent, formatRenewalUntil, formatRenewalWhen } from '@/lib/format'
import { useLocale } from '@/lib/locale'
import { currentUsage, quotaLabel, thresholds, trustedReset, usageLevel } from '@/providers/quotas'
import type { ProviderSnapshot, QuotaWindow } from '@/providers/types'

type Reading = { provider: ProviderSnapshot; quota: QuotaWindow; used: number }

export function UsageGuidance({ providers, now, refreshFailed, compact = false, dismissible = false }: {
  providers: ProviderSnapshot[]; now: number; refreshFailed: boolean; compact?: boolean; dismissible?: boolean
}) {
  const { t, language } = useLocale()
  const { dismissedGuidance: dismissed, setDismissedGuidance: setDismissed } = useDisplay()
  const checked = providers.filter(({ integration }) => integration === 'connected' || integration === 'demo')
  const readings = checked.map((provider) => {
    const known = provider.quotas.flatMap((quota): Reading[] => {
      const used = currentUsage(quota, now)
      return used === null ? [] : [{ provider, quota, used }]
    }).sort((a, b) => b.used - a.used)
    const hasPrimaryWindow = known.some(({ quota }) => !quota.scope && quota.type !== 'custom')
    const isBucketProvider = provider.quotas.length > 0 && provider.quotas.every(({ type }) => type === 'custom')
    return { known, complete: known.length === provider.quotas.length && (hasPrimaryWindow || isBucketProvider) }
  }).filter(({ known }) => known.length > 0)
  const byPressure = readings.map(({ known }) => known[0]).sort((a, b) => b.used - a.used)
  const concern = byPressure.find(({ used }) => used >= thresholds[0])
  const candidates = readings.filter(({ known: [highest], complete }) => complete && highest.used < thresholds[0])
    .map(({ known: [highest] }) => highest).sort((a, b) => a.used - b.used)
  const [alternative] = candidates
  const disconnected = providers.every(({ connectionSaved, integration }) => !connectionSaved && integration === 'not_connected')
  const available = !refreshFailed && byPressure.length > 0
  let tone = 'neutral'
  let title = t('Waiting for current usage')
  let detail = t('Your connected tools will show usage when the next reading arrives.')
  let suggestion: string | undefined
  // What makes a warning "the same one": once dismissed it stays hidden until the tool, limit, level or renewal changes.
  let identity = ''

  if (refreshFailed) {
    detail = t('Try updating again before deciding which tool to use.')
  } else if (disconnected) {
    title = t('Connect your first AI tool')
    detail = t(compact ? 'Open the dashboard, then choose Add AI tool.' : 'Choose Add AI tool to connect the AI tools on this computer.')
  } else if (available && concern) {
    const { provider, quota, used } = concern
    const level = usageLevel(used)
    identity = `${provider.providerId}|${quota.id}|${level.label}|${quota.resetAt ?? ''}`
    tone = level.tone
    title = t(level.label)
    const limit = quotaLabel(quota, provider.providerId, t)
    const at = trustedReset(quota)
    detail = used === 100
      ? t('{name} has used all of its allowance for {limit}.', { name: provider.displayName, limit })
      : t('{name} has {value}% left for {limit}.', { name: provider.displayName, limit, value: formatPercent(100 - used, language) })
    const nextStep = at === null
      ? t(used === 100 ? 'Check the tool to see when you can use it again.' : 'Save longer tasks for when more usage is available.')
      : used === 100
        ? t('You can wait until {time} for this limit to renew.', { time: formatRenewalUntil(at, now, language) })
        : t('This limit renews {time}.', { time: formatRenewalWhen(at, now, language) })
    suggestion = alternative ? t('For your next task, try {name}.', { name: alternative.provider.displayName }) : nextStep
    if (alternative && at !== null) detail = `${detail} ${t('This limit renews {time}.', { time: formatRenewalWhen(at, now, language) })}`
    if (byPressure.filter(({ used }) => used >= thresholds[0]).length > 1) {
      detail = `${detail} ${t('Check the other tools below for limits they are close to reaching.')}`
    }
  } else if (available && alternative) {
    tone = 'success'
    title = t('{name} has usage available', { name: alternative.provider.displayName })
    detail = t('You still have {value}% left on your closest limit ({limit}).', {
      value: formatPercent(100 - alternative.used, language),
      limit: quotaLabel(alternative.quota, alternative.provider.providerId, t),
    })
    if (candidates.length > 1) detail = `${detail} ${t('This tool has the most room among the tools checked.')}`
  } else if (available) {
    title = t('Some limits are still missing')
    detail = t('The limits we can see have room left. Open the tool to check the rest.')
  }

  identity ||= `${tone}|${title}`
  if (dismissible && dismissed === identity) return null
  const Icon = tone === 'success' ? CircleCheck : tone === 'neutral' ? Info : CircleAlert
  return <section className={`usage-guidance tone-${tone} notice-enter`} aria-label={t('What you can do now')}>
    <span className="guidance-icon"><Icon aria-hidden="true" /></span>
    <div>
      <h2>{title}</h2>
      <p>{detail}</p>
      {suggestion && <p className="guidance-action">{suggestion}</p>}
    </div>
    {dismissible && <Button variant="ghost" size="icon" className="guidance-dismiss" aria-label={t('Dismiss this notice')} title={t('Dismiss this notice')}
      onClick={() => setDismissed(identity)}><X aria-hidden="true" /></Button>}
  </section>
}
