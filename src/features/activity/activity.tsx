import { useLocale } from '@/lib/locale'
import { lazy, Suspense, useState } from 'react'
import { ChevronDown } from 'lucide-react'
import type { UseQueryResult } from '@tanstack/react-query'
import { PageHeading } from '@/components/shared/page-heading'
import { EmptyState, ErrorState, LoadingState, RefreshNotice } from '@/components/shared/states'
import { Button } from '@/components/ui/button'
import { useClock } from '@/hooks/use-clock'
import { useSessionHistory } from '@/hooks/use-usage'
import { useDisplay } from '@/lib/display'
import { desktop } from '@/services/desktop'
import { formatCompact, formatDateTime, formatNumber, formatPercent, formatUsd } from '@/lib/format'
import { summarizeCache, summarizeCost } from '@/providers/pricing'
import { providerName } from '@/providers/registry'
import type { UsageSnapshot } from '@/providers/types'
import {
  codexAccountUsage,
  dailyUsage,
  filterSessions,
  modelBreakdown,
  projectBreakdown,
  type Period,
  type SessionFilters,
} from './activity-data'
import { ActivityHeatmap } from './activity-heatmap'
import { MetricCard, SessionItem } from './session-list'
import { ShareStudio } from './share-studio'
import { UsageBreakdown } from './usage-breakdown'

const TokenChart = lazy(() => import('./token-chart'))
const initialFilters: SessionFilters = { period: 7, provider: '', model: '', project: '', branch: '' }

export function Activity({ query, demo, refresh }: { query: UseQueryResult<UsageSnapshot>; demo: boolean; refresh: () => Promise<unknown> }) {
  const { t } = useLocale()
  const { data, dataUpdatedAt, isPending, isError, isFetching } = query
  const history = useSessionHistory(desktop && !demo)
  const snapshot: UsageSnapshot | undefined = data
    ? {
      ...data,
      sessions: history.data ? history.data.sessions : data.sessions,
      historyError: history.data ? history.data.historyError : data.historyError,
    }
    : undefined
  const now = Math.max(useClock(true, 60_000), dataUpdatedAt, history.dataUpdatedAt)
  const providerOptions = new Map((snapshot?.sessions ?? []).map(({ providerId }) => [providerId, providerName(providerId)]))
  snapshot?.providers.forEach(({ providerId, displayName }) => providerOptions.set(providerId, displayName))

  return <>
    <PageHeading title="Activity" description="See when you used your AI tools, how efficiently they used cache and their API-equivalent estimate.">
      {snapshot && <ShareStudio sessions={snapshot.sessions} providers={[...providerOptions]} now={now} defaultPeriod={30} />}
    </PageHeading>
    {demo && <div className="page-notice"><strong>{t("Demo data")}</strong> · {t('This activity is an example.')}</div>}
    {(isPending || (desktop && !demo && history.isPending && !snapshot?.sessions.length)) && <LoadingState />}
    {isError && (snapshot ? <RefreshNotice retry={() => void refresh()} busy={isFetching} /> : <ErrorState retry={() => void refresh()} />)}
    {snapshot && <ActivityContent snapshot={snapshot} now={now} providerChoices={[...providerOptions]} />}
  </>
}

function ActivityContent({ snapshot, now, providerChoices }: { snapshot: UsageSnapshot; now: number; providerChoices: [string, string][] }) {
  const { t, language } = useLocale()
  const { activityPeriod, setActivityPeriod } = useDisplay()
  const [filters, setFilters] = useState<Omit<SessionFilters, 'period'>>({ provider: '', model: '', project: '', branch: '' })
  const activeFilters: SessionFilters = { ...filters, period: activityPeriod }
  const sessions = snapshot.sessions
  const matching = filterSessions(sessions, activeFilters, now)
  const known = matching.flatMap((session) => session.totalTokens === null ? [] : [session.totalTokens])
  const options = (key: 'model' | 'projectName' | 'gitBranch') => [...new Set(sessions.flatMap((session) => session[key] ?? []))].sort()
  const useAccount = Boolean(snapshot.codexDailyTokens?.length) && (!filters.provider || filters.provider === 'codex') && !filters.project && !filters.model && !filters.branch && known.length === 0
  const chart = useAccount ? codexAccountUsage(snapshot.codexDailyTokens ?? [], activityPeriod, now, language) : dailyUsage(matching, activityPeriod, now, language)
  const values = chart.flatMap(({ tokens }) => tokens === null ? [] : [tokens])
  const cost = summarizeCost(matching)
  const cache = summarizeCache(matching)
  const projects = projectBreakdown(matching, t('Unknown project'))
  const models = modelBreakdown(matching, t('Model unavailable'))
  const heatmapSource = filterSessions(sessions, { ...activeFilters, period: 90 }, now)

  return <>
    {snapshot.historyError && <p className="page-notice" role="status">{t("Couldn’t read local session history. Provider quotas and account totals are still available when reported.")}</p>}
    <div className="filters" aria-label={t("Session filters")}>
      <Filter label="Provider" value={filters.provider} onChange={(value) => setFilters({ ...filters, provider: value as SessionFilters['provider'] })} choices={providerChoices} all="All providers" />
      <Filter label="Model" value={filters.model} onChange={(model) => setFilters({ ...filters, model })} choices={options('model').map((model) => [model, model])} all="All models" />
      <Filter label="Project" value={filters.project} onChange={(project) => setFilters({ ...filters, project })} choices={options('projectName').map((project) => [project, project])} all="All projects" />
      <Filter label="Branch" value={filters.branch ?? ''} onChange={(branch) => setFilters({ ...filters, branch })} choices={options('gitBranch').map((branch) => [branch, branch])} all="All branches" />
    </div>
    <dl className="metrics">
      <MetricCard
        label="Sessions in period"
        value={sessions.length ? formatNumber(matching.length, language) : t('Unavailable')}
        hint={sessions.length ? t('{count} reporting providers', { count: new Set(matching.map((session) => session.providerId)).size }) : undefined}
      />
      <MetricCard
        label={useAccount ? 'Codex account tokens' : 'Known session tokens'}
        value={formatNumber(values.length ? values.reduce((sum, total) => sum + total, 0) : null, language)}
      />
      <MetricCard
        label="API-equivalent cost (est.)"
        value={formatUsd(cost, language)}
        hint={cost !== null ? t('Separate from subscription billing') : undefined}
      />
      <MetricCard
        label="Prompt cache efficiency"
        value={cache.hitRate !== null ? `${formatPercent(cache.hitRate, language)}%` : t('Unavailable')}
        hint={cache.cacheRead > 0
          ? cache.savedUsd !== null && cache.savedUsd >= 0.01
            ? t('{tokens} read · ~{saved} saved', { tokens: formatCompact(cache.cacheRead, language), saved: formatUsd(cache.savedUsd, language) })
            : t('{tokens} cached tokens read', { tokens: formatCompact(cache.cacheRead, language) })
          : undefined}
      />
    </dl>
    <section className="activity-chart panel card-enter" aria-labelledby="chart-title">
      <div className="section-heading">
        <div><h2 id="chart-title">{t('AI usage by day')}</h2><p>{t('Tokens are the small pieces of text your AI reads and writes.')}</p>
          {useAccount && <p>{t('OpenAI Codex account · Updated {time}', { time: snapshot.codexDailyUpdatedAt ? formatDateTime(snapshot.codexDailyUpdatedAt, language) : t('time unavailable') })}</p>}</div>
        <div className="segments" role="group" aria-label={t("Period")}>
          {([7, 30, 90] as Period[]).map((period) => <Button key={period} variant="ghost" aria-pressed={period === activityPeriod} onClick={() => setActivityPeriod(period)}>{t('{count} days', { count: period })}</Button>)}
        </div>
      </div>
      <Suspense fallback={<LoadingState />}><TokenChart data={chart} account={useAccount} /></Suspense>
    </section>
    <ActivityHeatmap sessions={heatmapSource} now={now} />
    {matching.length > 0 && <UsageBreakdown projects={projects} models={models} />}
    <section className="sessions" aria-labelledby="sessions-title">
      <div className="section-heading"><div><h2 id="sessions-title">{t("Sessions")}</h2><p>{t("Select a session to inspect its details.")}</p></div>{matching.length > 0 && <span className="section-counter">{t(matching.length === 1 ? '1 session' : '{count} sessions', { count: matching.length })}</span>}</div>
      {matching.length ? <ul className="session-list panel">{matching.map((session) => <SessionItem key={session.id} session={session} />)}</ul> :
        <EmptyState title={sessions.length ? 'No matching sessions' : 'No sessions available'} description={sessions.length ? 'Try another provider, model, project, branch, or period.' : snapshot.historyError ? 'Session history will return when local storage is readable.' : 'Session history will appear once a supported provider is connected.'}
          action={sessions.length > 0 && <Button variant="outline" onClick={() => { setFilters({ provider: '', model: '', project: '', branch: '' }); setActivityPeriod(initialFilters.period) }}>{t("Clear filters")}</Button>} />}
    </section>
  </>
}

function Filter({ label, value, onChange, choices, all }: { label: string; value: string; onChange: (value: string) => void; choices: [string, string][]; all: string }) {
  const { t } = useLocale()
  return <label className="filter"><span>{t(label)}</span><span className="select-wrap">
    <select value={value} onChange={(event) => onChange(event.target.value)}><option value="">{t(all)}</option>{choices.map(([id, name]) => <option key={id} value={id}>{name}</option>)}</select>
    <ChevronDown aria-hidden="true" />
  </span></label>
}
