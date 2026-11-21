import { ChevronDown, Ellipsis, RefreshCw, Trash2 } from 'lucide-react'
import { Dialog, DropdownMenu } from 'radix-ui'
import { useState } from 'react'
import { ProviderQuotaSkeleton, StatusIndicator } from '@/components/shared/states'
import { formatDateTime, formatPercent } from '@/lib/format'
import { currentUsage, isPercentage, quotaLabel, usageLevel } from '@/providers/quotas'
import type { IntegrationStatus, ProviderId, ProviderSnapshot } from '@/providers/types'
import { ProviderMark } from './provider-mark'
import { QuotaCard, useShareText } from './quota-card'
import { useLocale } from '@/lib/locale'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { desktop, refreshAntigravity, refreshClaude, refreshDesktop, removeProvider, type PreferencesState } from '@/services/desktop'

const isAntigravity = (id: ProviderId) => id === 'antigravity' || id === 'local-antigravity'

const integrationStatus = {
  demo: { label: 'Sample data', tone: 'accent' },
  connected: { label: 'Connected', tone: 'success' },
  stale: { label: 'Needs updated usage', tone: 'warning' },
  not_connected: { label: 'Not connected', tone: 'neutral' },
  error: { label: 'Connection issue', tone: 'danger' },
} as const

export function isInitialLoading(provider: ProviderSnapshot) {
  return Boolean(provider.connectionSaved && provider.integration === 'not_connected' && !provider.updatedAt && !provider.integrationError)
}

export function IntegrationIndicator({ status, saved, loading = false }: { status: IntegrationStatus; saved?: boolean; loading?: boolean }) {
  if (loading) return <StatusIndicator label="Loading…" tone="accent" />
  if (status === 'error') return <StatusIndicator {...integrationStatus.error} />
  if (saved && status !== 'connected' && status !== 'demo') return <StatusIndicator label="Connection saved" tone="accent" />
  return <StatusIndicator {...integrationStatus[status]} />
}

const refreshHelp: Record<string, string> = {
  claude: 'TempoAI updates usage automatically. You can also check it now.',
  antigravity: 'TempoAI updates usage automatically. You can also check it now.',
  'local-antigravity': 'TempoAI updates usage automatically. You can also check it now.',
}

export function showSavedReading(provider: ProviderSnapshot) {
  return provider.integration === 'stale' || (provider.connectionSaved &&
    (provider.integration === 'not_connected' || provider.integration === 'error'))
}

function useUsageCheck(providerId: ProviderId) {
  const client = useQueryClient()
  const mutationFn = isAntigravity(providerId)
    ? refreshAntigravity
    : providerId === 'claude'
      ? refreshClaude
      : refreshDesktop
  return useMutation<void, unknown>({
    mutationFn,
    onSettled: () => client.invalidateQueries({ queryKey: ['usage'] }),
  })
}

export function ProviderUsageAction({ provider }: { provider: ProviderSnapshot }) {
  const { t } = useLocale()
  const antigravity = isAntigravity(provider.providerId)
  const claude = provider.providerId === 'claude'
  const reconnecting = provider.integration === 'error' || provider.integration === 'not_connected'
  const check = useUsageCheck(provider.providerId)
  if (!desktop || provider.integration === 'demo' || (!antigravity && !claude && !reconnecting)) return null
  const label = reconnecting ? 'Try connecting again' : antigravity ? 'Check Antigravity usage' : 'Check Claude usage'
  const pendingLabel = reconnecting ? 'Connecting…' : 'Checking usage…'
  const fallback = antigravity
    ? 'Could not complete the Antigravity usage check.'
    : claude
      ? 'Could not complete the Claude usage check.'
      : 'Could not refresh usage.'
  const errorMessage = check.isError ? (typeof check.error === 'string' ? check.error : fallback) : null
  const alertText = errorMessage && errorMessage === provider.integrationError ? 'Still could not connect.' : errorMessage
  return <div className="provider-usage-action">
    <Button variant="outline" disabled={check.isPending} onClick={() => check.mutate()}>
      <RefreshCw className={check.isPending ? 'spin' : undefined} aria-hidden="true" />
      {t(check.isPending ? pendingLabel : label)}
    </Button>
    {alertText && <p role="alert">{t(alertText)}</p>}
    {antigravity && check.isSuccess && <p role="status">{t('Antigravity usage checked.')}</p>}
    {claude && check.isSuccess && <p role="status">{t('Claude usage checked.')}</p>}
  </div>
}

export function StaleProviderUsage({ provider }: { provider: ProviderSnapshot }) {
  const { t, language } = useLocale()
  const saved = provider.quotas.flatMap((quota) => isPercentage(quota.usedPercent) ? [{ quota, used: quota.usedPercent }] : [])
  const message = provider.integrationError
    ? t(provider.integrationError)
    : t(refreshHelp[provider.providerId] ?? 'Open the tool to check your current usage, then click Refresh in TempoAI.')
  return <div className="stale-provider" data-error={provider.integration === 'error' || undefined}>
    <p>{message}</p>
    {saved.length > 0 && <details>
      <summary>{t('See last reading')}</summary>
      {provider.updatedAt && <p>{t('Last observed {time}', { time: formatDateTime(provider.updatedAt, language) })}</p>}
      <ul>{saved.map(({ quota, used }) => <li key={quota.id}>
        <span>{quotaLabel(quota, provider.providerId, t)}</span>
        <span>{t('{value}% used', { value: formatPercent(used, language) })}</span>
      </li>)}</ul>
    </details>}
  </div>
}

/** Confirms before taking a tool off TempoAI. The tool itself and its account are not touched. */
export function RemoveProviderDialog({ provider, open, onOpenChange }: { provider: ProviderSnapshot; open: boolean; onOpenChange: (open: boolean) => void }) {
  const { t } = useLocale()
  const client = useQueryClient()
  const remove = useMutation<PreferencesState, unknown>({
    mutationFn: () => removeProvider(provider.providerId),
    onSuccess: async (saved) => {
      await client.cancelQueries({ queryKey: ['preferences'], exact: true })
      client.setQueryData(['preferences'], saved)
      onOpenChange(false)
      await client.invalidateQueries({ queryKey: ['usage'] })
    },
  })
  return <Dialog.Root open={open} onOpenChange={(next) => { if (!remove.isPending) { remove.reset(); onOpenChange(next) } }}>
    <Dialog.Portal>
      <Dialog.Overlay className="drawer-overlay" />
      <Dialog.Content className="confirm-dialog">
        <Dialog.Title>{t('Remove {name}?', { name: provider.displayName })}</Dialog.Title>
        <Dialog.Description>{t('TempoAI stops showing this tool. Your account and the tool itself stay as they are. You can add it again at any time.')}</Dialog.Description>
        {remove.isError && <p role="alert" className="form-error">{t(typeof remove.error === 'string' ? remove.error : 'Could not save preferences.')}</p>}
        <div className="dialog-actions">
          <Dialog.Close asChild><Button variant="ghost" disabled={remove.isPending}>{t('Cancel')}</Button></Dialog.Close>
          <Button variant="danger" disabled={remove.isPending} onClick={() => remove.mutate()}>{t(remove.isPending ? 'Removing…' : 'Remove')}</Button>
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
}

function ProviderMenu({ provider }: { provider: ProviderSnapshot }) {
  const { t } = useLocale()
  const [removing, setRemoving] = useState(false)
  const check = useUsageCheck(provider.providerId)
  return <>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <Button variant="ghost" size="icon" aria-label={t('Options for {name}', { name: provider.displayName })} title={t('More options')}><Ellipsis aria-hidden="true" /></Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu" align="end" sideOffset={6}>
          <DropdownMenu.Item className="menu-item" disabled={check.isPending} onSelect={() => check.mutate()}><RefreshCw aria-hidden="true" />{t('Check usage now')}</DropdownMenu.Item>
          <DropdownMenu.Separator className="menu-separator" />
          <DropdownMenu.Item className="menu-item menu-danger" onSelect={() => setRemoving(true)}><Trash2 aria-hidden="true" />{t('Remove from TempoAI')}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
    <RemoveProviderDialog provider={provider} open={removing} onOpenChange={setRemoving} />
  </>
}

/** Collapses or expands one tool. The label says what will happen, so screen readers announce the action. */
export function CollapseButton({ name, expanded, controls, onToggle }: { name: string; expanded: boolean; controls: string; onToggle: () => void }) {
  const { t } = useLocale()
  const label = t(expanded ? 'Collapse {name}' : 'Expand {name}', { name })
  return <Button variant="ghost" size="icon" className="collapse-button" aria-expanded={expanded} aria-controls={controls} aria-label={label} title={label} onClick={onToggle}>
    <ChevronDown aria-hidden="true" />
  </Button>
}

/** One line for a collapsed tool: its tightest limit, so the important number stays visible. */
export function CollapsedSummary({ provider, now }: { provider: ProviderSnapshot; now: number }) {
  const { t } = useLocale()
  const share = useShareText(provider.providerId)
  if (isInitialLoading(provider)) {
    return <p className="collapsed-summary">{t('Loading usage')}</p>
  }
  const tightest = provider.quotas.flatMap((quota) => {
    const used = currentUsage(quota, now)
    return used === null ? [] : [{ quota, used }]
  }).sort((a, b) => b.used - a.used)[0]
  if (!tightest) return <p className="collapsed-summary">{t(provider.integration === 'error' ? 'Connection issue' : showSavedReading(provider) ? 'Needs updated usage' : 'Usage data unavailable')}</p>
  const level = usageLevel(tightest.used)
  return <p className={`collapsed-summary tone-${level.tone}`}>
    <span>{quotaLabel(tightest.quota, provider.providerId, t)}</span><strong>{share(tightest.used)}</strong>
  </p>
}

export function ProviderCard({ provider, now, collapsed = false, onToggleCollapsed, index = 0 }: {
  provider: ProviderSnapshot; now: number; collapsed?: boolean; onToggleCollapsed?: () => void; index?: number
}) {
  const { t, language } = useLocale()
  const { integration } = provider
  const loading = isInitialLoading(provider)
  const body = `${provider.providerId}-details`
  return <article className="provider-card card-enter" style={{ animationDelay: `${Math.min(index, 6) * 40}ms` }} aria-label={provider.displayName} data-collapsed={collapsed || undefined}>
    <header className="provider-header">
      <ProviderMark id={provider.providerId} />
      <div className="provider-title"><h3>{provider.displayName}</h3>{provider.planName && <p>{t('{name} plan', { name: provider.planName })}</p>}</div>
      <IntegrationIndicator status={integration} saved={provider.connectionSaved} loading={loading} />
      {desktop && provider.connectionSaved && <ProviderMenu provider={provider} />}
      {onToggleCollapsed && <CollapseButton name={provider.displayName} expanded={!collapsed} controls={body} onToggle={onToggleCollapsed} />}
    </header>
    {collapsed ? <CollapsedSummary provider={provider} now={now} /> : <div id={body} className="provider-body body-reveal">
      {loading ? (
        <ProviderQuotaSkeleton />
      ) : showSavedReading(provider) ? <>
        <StaleProviderUsage provider={provider} />
        <ProviderUsageAction provider={provider} />
      </> : <>
        <div className="quota-list">{provider.quotas.map((quota) => <QuotaCard key={quota.id} quota={quota} provider={provider.providerId} now={now} />)}</div>
        <footer className="provider-footer">
          {provider.updatedAt ? t('Updated {time}', { time: formatDateTime(provider.updatedAt, language) }) :
            t(integration === 'not_connected' ? 'Connect this tool to see its usage.' : 'Usage data unavailable')}
        </footer>
      </>}
    </div>}
  </article>
}
