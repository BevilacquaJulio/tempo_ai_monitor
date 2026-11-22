import { Clock3, FlaskConical, RefreshCw } from 'lucide-react'
import { useEffect, useMemo, useRef, type ReactNode } from 'react'
import { Link } from 'react-router-dom'
import type { UseQueryResult } from '@tanstack/react-query'
import { ProviderCard } from '@/components/providers/provider-card'
import { RenewalTime } from '@/components/providers/quota-card'
import { PageHeading } from '@/components/shared/page-heading'
import { ErrorState, OverviewSkeleton, RefreshNotice } from '@/components/shared/states'
import { Button } from '@/components/ui/button'
import { useClock } from '@/hooks/use-clock'
import { formatTime } from '@/lib/format'
import { nextReset, quotaLabel, trustedReset } from '@/providers/quotas'
import type { UsageSnapshot } from '@/providers/types'
import { useLocale } from '@/lib/locale'
import { useCollapsedTools } from '@/lib/display'
import { desktop, type Preferences } from '@/services/desktop'
import { OnboardingScan } from './onboarding-scan'
import { UsageGuidance } from './usage-guidance'

export function Overview({ query, demo, refresh, scan, settings, saving = false }: {
  query: UseQueryResult<UsageSnapshot>
  demo: boolean
  refresh: () => Promise<unknown>
  scan: ReactNode
  settings?: Preferences
  saving?: boolean
}) {
  const { t, language } = useLocale()
  const { data, isFetching, isPending, isError } = query
  const collapsed = useCollapsedTools('overview')
  const resets = useMemo(() => data?.providers.flatMap((provider) => provider.quotas.flatMap((quota) => {
    const at = trustedReset(quota)
    return at === null ? [] : [{ id: `${provider.providerId}/${quota.id}/${at}`, at }]
  })) ?? [], [data])
  const hasActiveRetry = Boolean(data?.providers.some(({ waitingFor, nextRetryAt }) => waitingFor === 'retry' && Boolean(nextRetryAt)))
  const hasReset = Boolean(data?.providers.some(({ quotas }) => quotas.some(({ resetAt }) => resetAt)))
  const now = useClock(hasActiveRetry || hasReset, hasActiveRetry ? 1_000 : 30_000)
  const requested = useRef(new Set<string>())
  useEffect(() => {
    if (isFetching || isError) return
    const due = resets.filter(({ id, at }) => at <= now && !requested.current.has(id))
    if (!due.length) return
    due.forEach(({ id }) => requested.current.add(id))
    void refresh()
  }, [resets, now, isFetching, isError, refresh])
  const next = data && nextReset(data.providers, now)
  const firstRun = Boolean(desktop && !demo && data && data.providers.length === 0)

  return <>
    <PageHeading title="Overview" description="See how much you can still use your AI tools and when your limits renew.">
      <div className="sync-meta" aria-live="polite">
        <span>{isFetching ? t('Refreshing…') : data?.observedAt && t('Updated {time}', { time: formatTime(data.observedAt, language) })}</span>
      </div>
      <Button variant="outline" disabled={isFetching} onClick={() => void refresh()}><RefreshCw className={isFetching ? 'spin' : ''} aria-hidden="true" /> {t('Refresh')}</Button>
      {scan}
    </PageHeading>
    {demo && <div className="demo-banner"><FlaskConical aria-hidden="true" /><p><strong>{t('Demo data')}</strong><span>{t('Sample usage only. No provider accounts are connected.')}</span></p></div>}
    {isPending && <OverviewSkeleton />}
    {isError && (data ? <RefreshNotice retry={() => void refresh()} busy={isFetching} /> : <ErrorState retry={() => void refresh()} />)}
    {data && <>
      {firstRun
        ? <OnboardingScan settings={settings} saving={saving} />
        : <UsageGuidance providers={data.providers} now={now} refreshFailed={isError} />}
      {data.providers.length > 0 && <section aria-labelledby="providers-heading" className="overview-providers">
        <div className="section-heading">
          <div><h2 id="providers-heading">{t('Your AI tools')}</h2><p>{t('Each tool has its own limits. When a limit renews, you get more usage for that period.')}</p></div>
        </div>
        <div className="providers-grid">{data.providers.map((provider, index) => <ProviderCard key={provider.providerId} provider={provider} now={now} index={index}
          collapsed={collapsed.isCollapsed(provider.providerId)} onToggleCollapsed={() => collapsed.toggle(provider.providerId)} />)}</div>
      </section>}
      {next && !isError && <section className="next-reset card-enter" aria-label={t('Next renewal')}>
        <span className="next-icon"><Clock3 aria-hidden="true" /></span>
        <div><h2>{t('Next renewal')}</h2><p>{next.provider.displayName} <span> / </span> {quotaLabel(next.quota, next.provider.providerId, t)}</p></div>
        <RenewalTime at={next.at} now={now} />
      </section>}
      {data.providers.some(({ integration }) => integration === 'error') &&
        <p className="overview-note"><Link to="/settings">{t('Check tool connections in Settings')}</Link></p>}
    </>}
  </>
}
