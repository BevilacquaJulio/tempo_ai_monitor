import { CircleAlert, Inbox, LoaderCircle } from 'lucide-react'
import type { ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/locale'

export function EmptyState({ title, description, action }: { title: string; description: string; action?: ReactNode }) {
  const { t } = useLocale()
  return <div className="state-box"><span className="state-icon"><Inbox /></span><h3>{t(title)}</h3><p>{t(description)}</p>{action}</div>
}

export function LoadingState({ title = 'Loading usage' }: { title?: string }) {
  const { t } = useLocale()
  return <div className="state-box" role="status"><span className="state-icon"><LoaderCircle className="spin" /></span><h3>{t(title)}</h3><p>{t('Preparing the current view.')}</p></div>
}

export function ProviderQuotaSkeleton({ compact = false }: { compact?: boolean }) {
  const { t } = useLocale()
  return <div className={`quota-skeleton-stack${compact ? ' is-compact' : ''}`} role="status" aria-label={t('Loading usage')}>
    {[0, 1].map((idx) => (
      <div className="quota-skeleton-row" key={idx}>
        <div className="quota-skeleton-head">
          <span className="skel skel-label" />
          <span className="skel skel-pill" />
        </div>
        {!compact && <span className="skel skel-figure" />}
        <span className="skel skel-bar" />
        <span className="skel skel-meta" />
      </div>
    ))}
  </div>
}

export function OverviewSkeleton({ count = 2 }: { count?: number }) {
  const { t } = useLocale()
  return <div className="skeleton-overview" role="status" aria-label={t('Loading usage')}>
    <div className="skeleton-guidance">
      <span className="skel skel-icon" />
      <div className="skeleton-lines">
        <span className="skel skel-title" />
        <span className="skel skel-sub" />
      </div>
    </div>
    <div className="providers-grid">
      {Array.from({ length: count }, (_, index) => (
        <article className="provider-card skeleton-card" key={index} aria-hidden="true">
          <header className="provider-header">
            <span className="skel skel-mark" />
            <div className="skeleton-lines">
              <span className="skel skel-title" />
              <span className="skel skel-mini" />
            </div>
            <span className="skel skel-badge" />
          </header>
          <ProviderQuotaSkeleton />
        </article>
      ))}
    </div>
  </div>
}

export function QuickMonitorSkeleton({ count = 2 }: { count?: number }) {
  const { t } = useLocale()
  return <div className="skeleton-quick" role="status" aria-label={t('Loading usage')}>
    {Array.from({ length: count }, (_, index) => (
      <section className="quick-provider skeleton-card" key={index} aria-hidden="true">
        <header>
          <span className="skel skel-mark-sm" />
          <span className="skel skel-title" />
          <span className="skel skel-badge" />
        </header>
        <ProviderQuotaSkeleton compact />
      </section>
    ))}
  </div>
}

export function ErrorState({ retry, title = 'Couldn’t load usage' }: { retry: () => void; title?: string }) {
  const { t } = useLocale()
  return <div className="state-box" role="alert"><span className="state-icon tone-danger"><CircleAlert /></span><h3>{t(title)}</h3><p>{t('Try refreshing to request the data again.')}</p><Button variant="outline" onClick={retry}>{t('Try again')}</Button></div>
}

export function RefreshNotice({ retry, busy, message = 'Last refresh failed. Showing the last available data.' }: { retry: () => void; busy: boolean; message?: string }) {
  const { t } = useLocale()
  return <div className="page-notice refresh-notice">
    <p role="status">{t(message)}</p><Button variant="outline" disabled={busy} onClick={retry}>{t('Try again')}</Button>
  </div>
}

export function StatusIndicator({ label, tone = 'neutral' }: { label: string; tone?: 'neutral' | 'accent' | 'warning' | 'danger' | 'success' }) {
  const { t } = useLocale()
  return <span className={`status-indicator tone-${tone}`}><span aria-hidden="true" />{t(label)}</span>
}
