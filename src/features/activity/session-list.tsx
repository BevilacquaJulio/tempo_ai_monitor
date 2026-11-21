import { useLocale } from '@/lib/locale'
import { ArrowUpRight, X } from 'lucide-react'
import { Dialog } from 'radix-ui'
import { ProviderMark } from '@/components/providers/provider-mark'
import { Button } from '@/components/ui/button'
import { formatDateTime, formatDuration, formatNumber, formatPercent, formatUsd } from '@/lib/format'
import { estimateSessionCost, summarizeCache } from '@/providers/pricing'
import { providerName } from '@/providers/registry'
import type { SessionRecord } from '@/providers/types'

const sourceNames = { official: 'Official source', cli: 'Provider CLI', local: 'Local session', demo: 'Demo data' }

export function SessionItem({ session }: { session: SessionRecord }) {
  const { t, language } = useLocale()
  const project = session.projectName ?? t('Unknown project')
  const cost = estimateSessionCost(session)
  const cache = summarizeCache([session])
  return <li>
    <Dialog.Root>
      <Dialog.Trigger asChild>
        <button type="button" className="session-item" aria-label={t('View session details: {project}, {time}', { project, time: formatDateTime(session.lastActivityAt, language) })}>
          <ProviderMark id={session.providerId} />
          <span className="session-title">
            <strong>{project}</strong>
            <small>
              {providerName(session.providerId)} · {session.model ?? t('Model unavailable')}
              {session.gitBranch ? ` · ${session.gitBranch}` : ''}
            </small>
          </span>
          <time dateTime={session.lastActivityAt}>{formatDateTime(session.lastActivityAt, language)}</time>
          <span className="session-tokens">
            {session.totalTokens === null ? t('Unavailable') : `${formatNumber(session.totalTokens, language)} tokens`}
            {cost !== null && <small className="session-cost">{formatUsd(cost, language)}</small>}
          </span>
          <ArrowUpRight aria-hidden="true" />
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="drawer-overlay" />
        <Dialog.Content className="drawer">
          <header className="drawer-header"><div><Dialog.Title>{t("Session details")}</Dialog.Title><Dialog.Description>{t(sourceNames[session.source])}</Dialog.Description></div>
            <Dialog.Close asChild><Button variant="ghost" size="icon" title={t("Close details")} aria-label={t("Close details")}><X aria-hidden="true" /></Button></Dialog.Close>
          </header>
          <div className="drawer-project"><ProviderMark id={session.providerId} /><div><h3>{project}</h3><p>{providerName(session.providerId)}</p></div></div>
          <DetailRows rows={[
            ['Model', session.model ?? t('Unavailable')],
            ['Branch', session.gitBranch ?? t('Unavailable')],
            ['Requests', formatNumber(session.requestCount ?? null, language)],
            ['Started', session.startedAt ? formatDateTime(session.startedAt, language) : t('Unavailable')],
            ['Last activity', formatDateTime(session.lastActivityAt, language)],
            ['Duration', t(formatDuration(session.startedAt, session.lastActivityAt))],
            ['Last updated', formatDateTime(session.updatedAt, language)],
          ]} />
          <h3 className="detail-heading">{t("Token usage")}</h3>
          <DetailRows rows={[
            ['Input', formatNumber(session.inputTokens, language)],
            ['Output', formatNumber(session.outputTokens, language)],
            ['Cache read', formatNumber(session.cacheReadTokens ?? session.cacheTokens, language)],
            ['Cache creation', formatNumber(session.cacheCreationTokens ?? (session.totalTokens === null ? null : 0), language)],
            ['Cache hit rate', cache.hitRate !== null ? `${formatPercent(cache.hitRate, language)}%` : t('Unavailable')],
            ['Total', formatNumber(session.totalTokens, language)],
            ['API-equivalent cost (est.)', formatUsd(cost, language)],
          ]} />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  </li>
}

function DetailRows({ rows }: { rows: [string, string][] }) {
  const { t } = useLocale()
  return <dl className="detail-rows">{rows.map(([label, value]) => <div key={label}><dt>{t(label)}</dt><dd>{value}</dd></div>)}</dl>
}

export function MetricCard({ label, value, hint }: { label: string; value: string; hint?: string }) {
  const { t } = useLocale()
  return <div className="metric-card"><dt>{t(label)}</dt><dd>{value}</dd>{hint && <p className="metric-hint">{hint}</p>}</div>
}
