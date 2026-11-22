import { ChevronDown, Clock3, Ellipsis, LoaderCircle, RefreshCw, Trash2 } from 'lucide-react'
import { Dialog, DropdownMenu } from 'radix-ui'
import { useState } from 'react'
import { ProviderQuotaSkeleton, StatusIndicator } from '@/components/shared/states'
import { formatDateTime, formatPercent, formatTimeWithSeconds } from '@/lib/format'
import { countdown, currentUsage, isPercentage, isResetDue, quotaLabel, usageLevel } from '@/providers/quotas'
import type { IntegrationStatus, ProviderId, ProviderSnapshot } from '@/providers/types'
import { ProviderMark } from './provider-mark'
import { QuotaCard, useShareText } from './quota-card'
import { useLocale } from '@/lib/locale'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { desktop, refreshAntigravity, refreshClaude, refreshDesktop, removeProvider, type PreferencesState } from '@/services/desktop'

const isAntigravity = (id: ProviderId) => id === 'antigravity' || id === 'local-antigravity'

const CLAUDE_EXPIRED = 'Your Claude Code credentials expired. Start a Claude Code session (or open the CLI) and TempoAI will update automatically.'
const CLAUDE_SIGN_IN = 'Sign in to Claude Code with your Claude subscription to check usage here.'
const CLAUDE_BUSY = 'Claude is limiting usage checks right now. TempoAI will try again shortly.'

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

export function IntegrationIndicator({ status, saved, loading = false, checking = false }: {
  status: IntegrationStatus; saved?: boolean; loading?: boolean; checking?: boolean
}) {
  if (loading) return <StatusIndicator label="Loading…" tone="accent" />
  if (checking) return <StatusIndicator label="Checking…" tone="accent" />
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

function parseTimestamp(value: string | null | undefined): number | null {
  if (!value) return null
  const parsed = Date.parse(value)
  return Number.isFinite(parsed) ? parsed : null
}

function useUsageCheck(providerId: ProviderId, callbacks?: {
  onSuccess?: () => void
  onError?: (error: unknown) => void
}) {
  const client = useQueryClient()
  const mutationFn = isAntigravity(providerId)
    ? refreshAntigravity
    : providerId === 'claude'
      ? refreshClaude
      : refreshDesktop
  return useMutation<void, unknown>({
    mutationFn,
    onSuccess: () => callbacks?.onSuccess?.(),
    onError: (error) => callbacks?.onError?.(error),
    onSettled: () => client.invalidateQueries({ queryKey: ['usage'] }),
  })
}

export function ProviderUsageAction({ provider }: { provider: ProviderSnapshot }) {
  const { t, language } = useLocale()
  const antigravity = isAntigravity(provider.providerId)
  const claude = provider.providerId === 'claude'
  const reconnecting = provider.integration === 'error' || provider.integration === 'not_connected'
  const [feedback, setFeedback] = useState<{ kind: 'ok' | 'error'; text: string; count: number } | null>(null)
  const fallback = antigravity
    ? 'Could not complete the Antigravity usage check.'
    : claude
      ? 'Could not complete the Claude usage check.'
      : 'Could not refresh usage.'
  const check = useUsageCheck(provider.providerId, {
    onSuccess: () => {
      const time = formatTimeWithSeconds(Date.now(), language)
      const text = antigravity
        ? t('Checked at {time}: Antigravity usage updated.', { time })
        : claude
          ? t('Checked at {time}: Claude usage updated.', { time })
          : t('Checked at {time}: usage updated.', { time })
      setFeedback((prev) => ({ kind: 'ok', text, count: (prev?.count ?? 0) + 1 }))
    },
    onError: (error) => {
      const time = formatTimeWithSeconds(Date.now(), language)
      const raw = typeof error === 'string' ? error : fallback
      const text = raw === CLAUDE_EXPIRED
        ? t('Checked at {time}: Claude Code credentials are still expired. TempoAI will update as soon as Claude Code renews the session.', { time })
        : raw === CLAUDE_SIGN_IN
          ? t('Checked at {time}: no Claude Code sign-in found yet.', { time })
          : raw === CLAUDE_BUSY
            ? t('Checked at {time}: Claude is still limiting checks right now.', { time })
            : t('Checked at {time}: {reason}', { time, reason: t(raw) })
      setFeedback((prev) => ({ kind: 'error', text, count: (prev?.count ?? 0) + 1 }))
    },
  })
  const canCheck = antigravity || claude || reconnecting || provider.integration === 'stale'
  if (!desktop || provider.integration === 'demo' || !canCheck) return null
  const busy = check.isPending || Boolean(provider.checking)
  return <div className="provider-usage-action">
    <Button variant="outline" disabled={busy} onClick={() => check.mutate()}>
      <RefreshCw className={busy ? 'spin' : undefined} aria-hidden="true" />
      {t(busy ? 'Checking…' : 'Check now')}
    </Button>
    {feedback && <p key={feedback.count} className="action-feedback" role={feedback.kind === 'error' ? 'alert' : 'status'}>{feedback.text}</p>}
  </div>
}

export function StaleProviderUsage({ provider, now }: { provider: ProviderSnapshot; now: number }) {
  const { t, language } = useLocale()
  const saved = provider.quotas.flatMap((quota) => isPercentage(quota.usedPercent) ? [{ quota, used: quota.usedPercent }] : [])
  const message = provider.integrationError
    ? t(provider.integrationError)
    : t(refreshHelp[provider.providerId] ?? 'Open the tool to check your current usage, then click Refresh in TempoAI.')
  const retryAt = provider.waitingFor === 'retry' ? parseTimestamp(provider.nextRetryAt) : null
  const waitNotice = provider.checking
    ? { kind: 'checking' as const, text: t('Checking current usage…') }
    : provider.waitingFor === 'credentials'
      ? { kind: 'credentials' as const, text: t('Waiting for Claude Code to renew · checking automatically every 2s') }
      : provider.waitingFor === 'retry'
        ? {
            kind: 'retry' as const,
            text: retryAt !== null && retryAt > now
              ? t('Next automatic retry in {time}', { time: countdown(retryAt, now) })
              : t('Starting automatic retry…'),
          }
        : null

  return <div className="stale-provider" data-error={provider.integration === 'error' || undefined}>
    <p>{message}</p>
    {waitNotice && <p className={`wait-status is-${waitNotice.kind}`} role="status">
      {waitNotice.kind === 'checking'
        ? <LoaderCircle className="spin" aria-hidden="true" />
        : waitNotice.kind === 'retry'
          ? <Clock3 aria-hidden="true" />
          : <span className="wait-dot" aria-hidden="true" />}
      <span>{waitNotice.text}</span>
    </p>}
    {saved.length > 0 && <div className="last-reading">
      <p className="last-reading-head">
        {provider.updatedAt
          ? t('Last reading · {time}', { time: formatDateTime(provider.updatedAt, language) })
          : t('Last reading')}
      </p>
      <ul>{saved.map(({ quota, used }) => {
        const resetPassed = isResetDue(quota, now)
        return <li key={quota.id}>
          <span>{quotaLabel(quota, provider.providerId, t)}</span>
          <span className="last-reading-value">
            <strong>{t('{value}% used', { value: formatPercent(used, language) })}</strong>
            {resetPassed && <small>{t('Reset passed')}</small>}
          </span>
        </li>
      })}</ul>
    </div>}
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
      <IntegrationIndicator status={integration} saved={provider.connectionSaved} loading={loading} checking={Boolean(provider.checking)} />
      {desktop && provider.connectionSaved && <ProviderMenu provider={provider} />}
      {onToggleCollapsed && <CollapseButton name={provider.displayName} expanded={!collapsed} controls={body} onToggle={onToggleCollapsed} />}
    </header>
    {collapsed ? <CollapsedSummary provider={provider} now={now} /> : <div id={body} className="provider-body body-reveal">
      {loading ? (
        <ProviderQuotaSkeleton />
      ) : showSavedReading(provider) ? <>
        <StaleProviderUsage provider={provider} now={now} />
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
