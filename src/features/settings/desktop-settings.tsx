import { useLocale } from '@/lib/locale'
import { Dialog, Switch } from 'radix-ui'
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient, type UseQueryResult } from '@tanstack/react-query'
import { PageHeading } from '@/components/shared/page-heading'
import { ErrorState, LoadingState, RefreshNotice } from '@/components/shared/states'
import { IntegrationIndicator, isInitialLoading, RemoveProviderDialog } from '@/components/providers/provider-card'
import { ProviderMark } from '@/components/providers/provider-mark'
import { Button } from '@/components/ui/button'
import { formatDateTime } from '@/lib/format'
import { providerName } from '@/providers/registry'
import type { ProviderSnapshot, UsageSnapshot } from '@/providers/types'
import { clearHistory, getDataLocation, testSavedIntegrations, type PreferenceField, type Preferences, type PreferencesState } from '@/services/desktop'
import { AppearanceSection, SettingRow, SettingsSection } from './settings'
import type { usePreferencesDraft } from './use-preferences-draft'

type Editor = ReturnType<typeof usePreferencesDraft>
type Props = { preferences: UseQueryResult<PreferencesState>; editor: Editor; data?: UsageSnapshot; reloadUsage: () => Promise<unknown>; scan: ReactNode }
type Flag = { [K in keyof Preferences]-?: NonNullable<Preferences[K]> extends boolean ? K : never }[keyof Preferences]
const alertOptions = [
  [80, 'About 20% left'], [90, 'About 10% left'], [95, 'About 5% left'], [100, 'Limit reached'],
] as const

export function DesktopSettings({ preferences, editor, data, reloadUsage, scan }: Props) {
  return <>
    <PageHeading title="Settings" description="Choose how TempoAI looks, which tools it shows and when it warns you." />
    {preferences.isPending && <LoadingState title="Loading preferences" />}
    {preferences.isError && (preferences.data
      ? <RefreshNotice retry={() => void preferences.refetch()} busy={preferences.isFetching} message="Couldn’t refresh preferences. Showing the last loaded settings." />
      : <ErrorState retry={() => void preferences.refetch()} title="Couldn’t load preferences" />)}
    {preferences.data && <PreferencesForm state={preferences.data} editor={editor} data={data} reloadUsage={reloadUsage} scan={scan} />}
  </>
}

function PreferencesForm({ state, editor, data, reloadUsage, scan }: { state: PreferencesState; editor: Editor; data?: UsageSnapshot; reloadUsage: () => Promise<unknown>; scan: ReactNode }) {
  const { t } = useLocale()
  const { draft, set, save, error, discard } = editor
  const [notice, setNotice] = useState('')
  const [confirm, setConfirm] = useState(false)
  const form = useRef<HTMLFormElement>(null)
  const dirty = Object.keys(draft).length > 0
  const settings = { ...state.settings, ...draft }
  const client = useQueryClient()
  const location = useQuery({ queryKey: ['history-location'], queryFn: getDataLocation, retry: false })
  const clear = useMutation({
    mutationFn: clearHistory,
    onSuccess: async () => {
      setConfirm(false)
      setNotice('Activity history cleared.')
      await client.invalidateQueries({ queryKey: ['sessions'] })
      await reloadUsage()
    },
  })
  useEffect(() => {
    if (!error?.field || save.isPending) return
    requestAnimationFrame(() => form.current?.querySelector<HTMLInputElement>(`input[name="${error.field}"]`)?.focus())
  }, [error, save.isPending])
  const toggle = (key: Flag, label: string, description?: string) => (
    <SettingRow title={label} description={description} controlId={key} value={
      <Switch.Root className="switch" id={key} checked={Boolean(settings[key])} onCheckedChange={(value) => set(key, value)}><Switch.Thumb /></Switch.Root>
    } />
  )
  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (save.isPending || (!dirty && !state.shortcutConflict)) return
    setNotice('')
    save.mutate(settings)
  }
  const { isSuccess: saved, reset: clearSaved } = save
  useEffect(() => {
    if (!saved) return
    const timer = setTimeout(clearSaved, 3000)
    return () => clearTimeout(timer)
  }, [saved, clearSaved])
  const fieldError = (field: PreferenceField) => error?.field === field ? error.message : undefined
  const connected = data?.providers.filter(({ connectionSaved }) => connectionSaved) ?? []
  const alertTools = [
    settings.claudeEnabled && { key: 'claudeAlerts' as const, label: 'Claude Code', checked: settings.claudeAlerts },
    settings.codexEnabled && { key: 'codexAlerts' as const, label: 'OpenAI Codex', checked: settings.codexAlerts },
    settings.antigravityEnabled && { key: 'antigravityAlerts' as const, label: 'Google Antigravity', checked: settings.antigravityAlerts ?? true },
  ].filter((item): item is { key: 'claudeAlerts' | 'codexAlerts' | 'antigravityAlerts'; label: string; checked: boolean } => Boolean(item))

  return <form ref={form} onSubmit={submit} className="settings-form">
    {state.recoveryWarning && <p role="status" className="page-notice">{t(state.recoveryWarning)}</p>}
    <fieldset aria-label={t('Desktop preferences')} className="settings-stack" disabled={save.isPending}>
      <AppearanceSection />

      <SettingsSection title="Your AI tools" description="The tools TempoAI shows on the Overview.">
        {connected.length ? connected.map((provider) => <ToolRow key={provider.providerId} provider={provider} />)
          : <p className="settings-empty">{t('No tools yet. Add one to start seeing how much you have left.')}</p>}
        <div className="settings-card-actions">{scan}<IntegrationCheck data={data} state={state} reloadUsage={reloadUsage} /></div>
      </SettingsSection>

      <SettingsSection title="Alerts" description="Get a Windows notification before you run out.">
        {toggle('notifications', 'Warn me when a limit is close')}
        <fieldset className="setting-row thresholds" disabled={!settings.notifications}>
          <legend className="setting-label"><span>{t('When to warn me')}</span></legend>
          <div className="chip-group">{alertOptions.map(([level, label]) => <label key={level} className="chip">
            <input type="checkbox" checked={settings.thresholds.includes(level)} onChange={(event) => {
              set('thresholds', event.target.checked ? [...settings.thresholds, level].sort((a, b) => a - b) : settings.thresholds.filter((item) => item !== level))
            }} /><span>{t(label)}</span>
          </label>)}</div>
        </fieldset>
        {alertTools.length > 0 && (
          <fieldset className="setting-row thresholds" disabled={!settings.notifications}>
            <legend className="setting-label"><span>{t('Tools with alerts')}</span></legend>
            <div className="chip-group">
              {alertTools.map((tool) => (
                <label key={tool.key} className="chip">
                  <input type="checkbox" checked={tool.checked} onChange={(event) => set(tool.key, event.target.checked)} />
                  <span>{tool.label}</span>
                </label>
              ))}
            </div>
          </fieldset>
        )}
      </SettingsSection>

      <SettingsSection title="Windows" description="How TempoAI starts and stays available.">
        {toggle('autostart', 'Start with Windows')}
        {toggle('minimizeToTray', 'Keep running when dashboard closes', 'TempoAI stays next to the clock so alerts keep working.')}
        {toggle('openPanelAtStartup', 'Open Quick Monitor at startup')}
        {toggle('pinPanel', 'Keep Quick Monitor open when it loses focus', 'Also keeps Quick Monitor open when you open the dashboard.')}
        <SettingRow title="Quick Monitor shortcut" value={
          <TextField name="shortcut" label="Quick Monitor shortcut" hideLabel placeholder="Ctrl+Alt+W" value={settings.shortcut} onChange={(value) => set('shortcut', value)}
            error={fieldError('shortcut') ?? (state.shortcutConflict && draft.shortcut === undefined ? 'The saved shortcut could not be registered. Enter a different combination.' : undefined)} />
        } />
      </SettingsSection>

      <SettingsSection title="Your data" description="TempoAI keeps usage numbers and session names on this computer only. It never saves your conversations or passwords.">
        <SettingRow title="Activity history" description="Clearing it does not change anything in your AI tools." value={
          <Dialog.Root open={confirm} onOpenChange={(open) => { if (!clear.isPending) setConfirm(open) }}>
            <Dialog.Trigger asChild><Button variant="outline">{t('Clear history')}</Button></Dialog.Trigger>
            <Dialog.Portal>
              <Dialog.Overlay className="drawer-overlay" />
              <Dialog.Content className="confirm-dialog">
                <Dialog.Title>{t('Clear activity history?')}</Dialog.Title>
                <Dialog.Description>{t('TempoAI removes the sessions it has recorded. Your AI tools keep their own data. New activity is recorded from now on.')}</Dialog.Description>
                {clear.isError && <p role="alert" className="form-error">{t(String(clear.error))}</p>}
                <div className="dialog-actions">
                  <Dialog.Close asChild><Button variant="ghost" disabled={clear.isPending}>{t('Cancel')}</Button></Dialog.Close>
                  <Button variant="danger" onClick={() => clear.mutate()} disabled={clear.isPending}>{t('Clear history')}</Button>
                </div>
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
        } />
        {location.data && <SettingRow title="History file" value={<code className="path-note">{location.data}</code>} />}
        {notice && <p role="status" className="settings-note">{t(notice)}</p>}
      </SettingsSection>
    </fieldset>

    <div className="save-bar" data-visible={dirty || state.shortcutConflict || save.isPending || Boolean(error && !error.field) || save.isSuccess || undefined} role="region" aria-label={t('Save changes')}>
      <p role="status">{save.isPending ? t('Saving preferences…') : error && !error.field ? <span className="form-error">{t(error.message)}</span>
        : dirty ? t('Unsaved changes') : save.isSuccess ? t('Preferences saved.') : null}</p>
      {dirty && <Button variant="ghost" disabled={save.isPending} onClick={discard}>{t('Discard')}</Button>}
      {(dirty || state.shortcutConflict || save.isPending) && <Button type="submit" disabled={save.isPending}>{t('Save changes')}</Button>}
    </div>
  </form>
}

function ToolRow({ provider }: { provider: ProviderSnapshot }) {
  const { t, language } = useLocale()
  const [removing, setRemoving] = useState(false)
  const loading = isInitialLoading(provider)
  const detail = provider.integrationError ? t(provider.integrationError)
    : provider.updatedAt ? t('Updated {time}', { time: formatDateTime(provider.updatedAt, language) }) : t('Waiting for the first reading')
  return <div className="tool-row">
    <ProviderMark id={provider.providerId} />
    <div className="tool-name"><strong>{provider.displayName}</strong><span className={provider.integrationError ? 'form-error' : undefined}>{detail}</span></div>
    <IntegrationIndicator status={provider.integration} saved={provider.connectionSaved} loading={loading} />
    <Button variant="ghost" onClick={() => setRemoving(true)} aria-label={t('Remove {name}', { name: provider.displayName })}>{t('Remove')}</Button>
    <RemoveProviderDialog provider={provider} open={removing} onOpenChange={setRemoving} />
  </div>
}

function IntegrationCheck({ data, state, reloadUsage }: { data?: UsageSnapshot; state: PreferencesState; reloadUsage: () => Promise<unknown> }) {
  const { t } = useLocale()
  const check = useMutation({ mutationFn: testSavedIntegrations, onSuccess: () => reloadUsage() })
  const name = (id: string) => data?.providers.find((provider) => provider.providerId === id)?.displayName
    ?? state.settings.localProviders?.find((source) => source.id === id)?.name ?? providerName(id as ProviderSnapshot['providerId'])
  return <>
    <Button variant="outline" disabled={check.isPending} onClick={() => check.mutate()}>{t('Check connections')}</Button>
    <div role="status" className="check-results">
      {check.isPending && <p>{t('Checking connections…')}</p>}
      {check.data && <ul>{check.data.map(({ providerId, status, message }) => <li key={providerId} className={status === 'error' ? 'form-error' : undefined}>
        <strong>{name(providerId)}</strong>: {t(message)}
      </li>)}</ul>}
      {check.isError && <p role="alert" className="form-error">{t('Could not check connections. Try again.')}</p>}
    </div>
  </>
}

function TextField({ name, label, value, placeholder, error, hideLabel, onChange }: {
  name: PreferenceField; label: string; value: string; placeholder: string; error?: string; hideLabel?: boolean; onChange: (value: string) => void
}) {
  const { t } = useLocale()
  return <div className="text-field">
    <label htmlFor={name} className={hideLabel ? 'sr-only' : undefined}>{t(label)}</label>
    <input id={name} name={name} type="text" value={value} placeholder={t(placeholder)} aria-invalid={error ? true : undefined}
      aria-describedby={error ? `${name}-error` : undefined} onChange={(event) => onChange(event.target.value)} />
    {error && <p id={`${name}-error`} className="form-error">{t(error)}</p>}
  </div>
}
