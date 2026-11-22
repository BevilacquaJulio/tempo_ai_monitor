import { Plus } from 'lucide-react'
import { Dialog } from 'radix-ui'
import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/locale'
import { providerNames } from '@/providers/registry'
import { connectProvider, desktop, scanProviders, type DetectedAgent, type LocalProvider, type Preferences, type ProviderConnection } from '@/services/desktop'
import { isAgentEnabled, isNativeZeroConfig, nativeConnectionDescription, zeroConfigConnection } from './provider-connection'
import { LocalProviderForm } from './provider-source-form'

type Props = { settings?: Preferences; saving: boolean }

function connectionError(error: unknown) {
  return typeof error === 'string' ? error : 'Could not save preferences.'
}

export function ProviderScan({ settings, saving }: Props) {
  const { t } = useLocale()
  const navigate = useNavigate()
  const [open, setOpen] = useState(false)
  const [choice, setChoice] = useState('')
  const scan = useMutation({ mutationFn: scanProviders })
  const client = useQueryClient()
  const connection = useMutation({
    mutationFn: connectProvider,
    onSuccess: async (saved) => {
      await client.cancelQueries({ queryKey: ['preferences'], exact: true })
      client.setQueryData(['preferences'], saved)
      void client.invalidateQueries({ queryKey: ['usage'], exact: true })
    },
  })
  const agent = scan.data?.agents.find(({ id }) => id === choice)
  const existingSource = agent && settings?.localProviders?.find(({ id }) => id === `local-${agent.id}`)
  const nativePayload = agent ? zeroConfigConnection(agent) : null
  const needsFile = choice === 'custom' || Boolean(agent && !nativePayload)
  const busy = saving || connection.isPending
  const select = (id: string) => { connection.reset(); setChoice(id) }
  const addSource = (name: string, path: string) => {
    const id: LocalProvider['id'] = agent ? `local-${agent.id}` : `local-${crypto.randomUUID()}`
    connection.mutate({ kind: 'local', source: { id, name, path } })
  }

  return <Dialog.Root open={open} onOpenChange={(next) => {
    if (connection.isPending) return
    setOpen(next)
    if (next) {
      select('')
      if (desktop) scan.mutate()
    }
  }}>
    <Dialog.Trigger asChild><Button disabled={saving}><Plus aria-hidden="true" />{t('Add AI tool')}</Button></Dialog.Trigger>
    <Dialog.Portal>
      <Dialog.Overlay className="drawer-overlay" />
      <Dialog.Content className="confirm-dialog scan-dialog">
        <Dialog.Title>{t('Add AI tool')}</Dialog.Title>
        <Dialog.Description>{t('TempoAI looks for AI tools on this computer so you can see how much you have left to use.')}</Dialog.Description>
        {!desktop ? <p className="page-notice">{t('Open the TempoAI Windows app to scan installed agents.')}</p> : <>
          {scan.isPending && <p role="status">{t('Scanning this computer…')}</p>}
          {scan.isError && <p role="alert" className="form-error">{t('Could not scan for providers.')}</p>}
          {scan.isSuccess && !scan.data.agents.length && <p>{t('We didn’t find a supported AI tool here. If you just installed one, try searching again.')}</p>}
          {(scan.isSuccess || scan.isError) && <fieldset className="scan-options" disabled={busy}>
            <legend>{t('Choose an AI tool')}</legend>
            {scan.data?.agents.map((item) => <label key={item.id}>
              <input type="radio" name="scan-provider" checked={choice === item.id} onChange={() => select(item.id)} />
              <span><strong>{providerNames[item.id]}</strong><small>{t(
                isNativeZeroConfig(item) ? 'Ready · Zero-config' : 'Installation detected',
              )}</small></span>
            </label>)}
            <label><input type="radio" name="scan-provider" checked={choice === 'custom'} onChange={() => select('custom')} />
              <span><strong>{t('Other provider')}</strong><small>{t('Connect a local usage file')}</small></span>
            </label>
          </fieldset>}
          {nativePayload && agent && <NativeSetup agent={agent} payload={nativePayload}
            settings={settings} saving={busy || connection.isSuccess} onConnect={connection.mutate} />}
          {needsFile && <LocalProviderForm key={choice} name={existingSource?.name ?? (agent && providerNames[agent.id])}
            path={existingSource?.path}
            ready={Boolean(settings) && !busy && !connection.isSuccess}
            atLimit={(settings?.localProviders?.length ?? 0) >= 20 && !existingSource} onReview={addSource} />}
          {!settings && (nativePayload || needsFile) && <p>{t('Load preferences in Settings before adding a provider.')}</p>}
          {connection.isPending && <p role="status">{t('Saving provider…')}</p>}
          {connection.isError && <p role="alert" className="form-error">{t(connectionError(connection.error))}</p>}
          {connection.isSuccess && <p role="status">{t(agent?.id === 'antigravity' ? 'Antigravity stays connected and TempoAI checks its usage automatically.' : 'Tool saved. Go back to Overview to check its usage.')}</p>}
        </>}
        <div className="dialog-actions">
          {desktop && <Button variant="outline" disabled={scan.isPending || busy} onClick={() => { select(''); scan.mutate() }}>{t('Scan again')}</Button>}
          {connection.isSuccess && <Button variant="outline" onClick={() => { setOpen(false); void navigate('/settings') }}>{t('Settings')}</Button>}
          <Dialog.Close asChild><Button variant="ghost" disabled={busy}>{t('Close')}</Button></Dialog.Close>
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
}

function NativeSetup({ agent, payload, settings, saving, onConnect }: {
  agent: DetectedAgent; payload: ProviderConnection; settings?: Preferences; saving: boolean; onConnect: (connection: ProviderConnection) => void
}) {
  const { t } = useLocale()
  const enabled = isAgentEnabled(agent, settings)
  return <section className="scan-setup body-reveal" aria-label={providerNames[agent.id]}>
    <h3>{t('Recommended connection')}</h3>
    <p>{t(nativeConnectionDescription(agent))}</p>
    {agent.sourcePath && <p><span>{t('Executable path')}: </span><code className="path-note">{agent.sourcePath}</code></p>}
    {enabled && <p>{t('This provider is enabled in your preferences.')}</p>}
    <Button disabled={!settings || saving} onClick={() => onConnect(payload)}>{t('Connect provider')}</Button>
  </section>
}
