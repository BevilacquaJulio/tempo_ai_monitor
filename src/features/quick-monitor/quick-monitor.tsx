import { useLocale } from '@/lib/locale'
import { listen } from '@tauri-apps/api/event'
import { ArrowUpRight, Pin, RefreshCw, Settings, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { ProviderMark } from '@/components/providers/provider-mark'
import { CollapseButton, CollapsedSummary, IntegrationIndicator, isInitialLoading, ProviderUsageAction, showSavedReading, StaleProviderUsage } from '@/components/providers/provider-card'
import { ShareToggle, UsageGauge } from '@/components/providers/quota-card'
import { ErrorState, ProviderQuotaSkeleton, QuickMonitorSkeleton, RefreshNotice } from '@/components/shared/states'
import { useClock } from '@/hooks/use-clock'
import { useUsage } from '@/hooks/use-usage'
import { formatPercent, formatRenewal, formatTime } from '@/lib/format'
import { currentUsage, isPercentage, isResetDue, quotaLabel, trustedReset, usageLevel } from '@/providers/quotas'
import { desktop, getPreferences, hidePanel, openDashboard, setPanelPin } from '@/services/desktop'
import { UsageGuidance } from '@/features/overview/usage-guidance'
import { useCollapsedTools } from '@/lib/display'

export function QuickMonitor() {
  const { t, language } = useLocale()
  const collapsed = useCollapsedTools('quick')
  const { query, refresh } = useUsage()
  const { data, isPending, isError } = query
  const preferences = useQuery({ queryKey: ['preferences'], queryFn: getPreferences, retry: false })
  const [openPulse, setOpenPulse] = useState(0)
  const { mutate: pin, isPending: saving, isError: saveError } = useMutation({
    mutationFn: setPanelPin,
    onSuccess: () => preferences.refetch(),
  })
  const settings = preferences.data?.settings
  const now = useClock(Boolean(data?.providers.some(({ quotas }) => quotas.some(({ resetAt }) => resetAt))), 30_000)
  const refetchPreferences = preferences.refetch

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !event.defaultPrevented && !event.repeat) void hidePanel()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  useEffect(() => {
    if (!desktop) return
    const stopPrefs = listen('preferences-updated', () => void refetchPreferences())
    const stopOpen = listen('panel-opened', () => setOpenPulse((count) => count + 1))
    void stopPrefs.catch(() => {})
    void stopOpen.catch(() => {})
    return () => {
      void stopPrefs.then((unlisten) => unlisten()).catch(() => {})
      void stopOpen.then((unlisten) => unlisten()).catch(() => {})
    }
  }, [refetchPreferences])

  return <main className="quick-monitor panel-enter" key={openPulse} aria-label={t("Quick Monitor")}>
    {/* The header is the window's title bar: drag it to move Quick Monitor. Each child needs the attribute too. */}
    <header className="quick-header" data-tauri-drag-region title={t('Drag to move')}>
      <img src="./icone-tempoai-fundo.png" alt="" width="26" height="26" data-tauri-drag-region />
      <div data-tauri-drag-region><h1 data-tauri-drag-region>{t("Quick Monitor")}</h1><p data-tauri-drag-region>{t('Drag to move')}</p></div>
      <Button variant="ghost" size="icon" className="pin-button" title={t("Pin panel")} aria-label={t("Pin panel")} aria-pressed={settings?.pinPanel ?? false} disabled={!settings || saving}
        onClick={() => { if (settings) pin(!settings.pinPanel) }}><Pin aria-hidden="true" /></Button>
      <Button variant="ghost" size="icon" title={t("Close panel")} aria-label={t("Close panel")} onClick={() => void hidePanel()}><X aria-hidden="true" /></Button>
    </header>
    <div className="quick-content">
      {saveError && <p role="alert" className="form-error">{t("Could not save the pin preference.")}</p>}
      {isPending && <QuickMonitorSkeleton />}
      {isError && (data ? <RefreshNotice retry={() => void refresh()} busy={query.isFetching} /> : <ErrorState retry={() => void refresh()} />)}
      {data && <UsageGuidance providers={data.providers} now={now} refreshFailed={isError} compact dismissible />}
      {data?.providers.map((provider, index) => {
        const closed = collapsed.isCollapsed(provider.providerId)
        const loading = isInitialLoading(provider)
        const body = `quick-${provider.providerId}`
        return <section className="quick-provider card-enter" style={{ animationDelay: `${Math.min(index, 5) * 35}ms` }} key={provider.providerId} aria-label={provider.displayName} data-collapsed={closed || undefined}>
          <header><ProviderMark id={provider.providerId} /><h2>{provider.displayName}</h2><IntegrationIndicator status={provider.integration} saved={provider.connectionSaved} loading={loading} />
            <CollapseButton name={provider.displayName} expanded={!closed} controls={body} onToggle={() => collapsed.toggle(provider.providerId)} /></header>
          {closed ? <CollapsedSummary provider={provider} now={now} /> : <div id={body} className="body-reveal">
          {loading ? (
            <ProviderQuotaSkeleton compact />
          ) : showSavedReading(provider) ? <StaleProviderUsage provider={provider} /> : provider.quotas.map((quota) => {
            const reset = trustedReset(quota)
            const label = quotaLabel(quota, provider.providerId, t)
            const used = currentUsage(quota, now)
            const due = isResetDue(quota, now)
            return <div className="quick-quota" key={quota.id}>
              <div><span>{label}</span>{used !== null ? <ShareToggle used={used} toolId={provider.providerId} className="quick-figure" />
                : <strong>{(quota.status === 'stale' || due) && isPercentage(quota.usedPercent)
                  ? t('Last reading: {value}% used', { value: formatPercent(quota.usedPercent, language) })
                  : t(due ? 'Awaiting update' : quota.status === 'stale' ? 'Out of date' : 'Unavailable')}</strong>}</div>
              {used !== null && <UsageGauge used={used} label={`${provider.displayName} ${label}`} tone={usageLevel(used).tone} toolId={provider.providerId} />}
              {reset !== null && reset > now && <time className="countdown" dateTime={quota.resetAt ?? undefined}>{formatRenewal(reset, now, language)}</time>}
            </div>
          })}
          {!loading && showSavedReading(provider) && <ProviderUsageAction provider={provider} />}
          </div>}
        </section>
      })}
    </div>
    <footer className="quick-footer">
      <div><span>{data?.observedAt ? t('Updated {time}', { time: formatTime(data.observedAt, language) }) : t('No sync yet')}</span>
        <Button variant="ghost" size="icon" title={t("Refresh")} aria-label={t("Refresh")} disabled={query.isFetching} onClick={() => void refresh()}><RefreshCw className={query.isFetching ? 'spin' : ''} aria-hidden="true" /></Button></div>
      <div><Button variant="outline" size="icon" title={t("Settings")} aria-label={t("Settings")} onClick={() => void openDashboard(true)}><Settings aria-hidden="true" /></Button>
        <Button variant="outline" onClick={() => void openDashboard()}>{t("Open Dashboard")} <ArrowUpRight aria-hidden="true" /></Button></div>
    </footer>
  </main>
}
