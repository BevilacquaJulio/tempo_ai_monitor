import { Check, CheckCheck, FileJson, Plus, Radar, RefreshCw, ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { ProviderMark } from '@/components/providers/provider-mark'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/locale'
import { providerNames } from '@/providers/registry'
import {
  connectProvider,
  scanProviders,
  type DetectedAgent,
  type LocalProvider,
  type Preferences,
  type ProviderConnection,
} from '@/services/desktop'
import { agentSignals, zeroConfigConnection } from './provider-connection'
import { LocalProviderForm } from './provider-source-form'

type Props = {
  settings?: Preferences
  saving: boolean
}

export function OnboardingScan({ settings, saving }: Props) {
  const { t } = useLocale()
  const client = useQueryClient()
  const [addedIds, setAddedIds] = useState<string[]>([])
  const [connectingId, setConnectingId] = useState<string | null>(null)
  const [connectError, setConnectError] = useState<string | null>(null)
  const [manualAgent, setManualAgent] = useState<DetectedAgent | 'custom' | null>(null)
  const [showManualOnly, setShowManualOnly] = useState(false)

  const applySaved = async (saved: Awaited<ReturnType<typeof connectProvider>>) => {
    await client.cancelQueries({ queryKey: ['preferences'], exact: true })
    client.setQueryData(['preferences'], saved)
    void client.invalidateQueries({ queryKey: ['usage'], exact: true })
  }

  const scan = useMutation({ mutationFn: scanProviders })

  const phase: 'idle' | 'scanning' | 'results' = scan.isPending
    ? 'scanning'
    : scan.isSuccess || scan.isError || showManualOnly
      ? 'results'
      : 'idle'

  const runScan = () => {
    setConnectError(null)
    setManualAgent(null)
    setShowManualOnly(false)
    scan.mutate()
  }

  const connectOne = async (id: string, connection: ProviderConnection) => {
    setConnectingId(id)
    setConnectError(null)
    try {
      const saved = await connectProvider(connection)
      setAddedIds((prev) => (prev.includes(id) ? prev : [...prev, id]))
      await applySaved(saved)
    } catch (error) {
      setConnectError(typeof error === 'string' ? error : 'Could not save preferences.')
    } finally {
      setConnectingId(null)
    }
  }

  const result = scan.data ?? null
  const readyAgents = result
    ? result.agents.flatMap((agent) => {
      const conn = zeroConfigConnection(agent)
      return conn ? [{ agent, conn }] : []
    })
    : []

  const pendingReady = readyAgents.filter(({ agent }) => !addedIds.includes(agent.id))

  const connectAll = async () => {
    if (!pendingReady.length) return
    setConnectingId('__all__')
    setConnectError(null)
    try {
      let lastSaved: Awaited<ReturnType<typeof connectProvider>> | null = null
      for (const { agent, conn } of pendingReady) {
        lastSaved = await connectProvider(conn)
        setAddedIds((prev) => (prev.includes(agent.id) ? prev : [...prev, agent.id]))
      }
      if (lastSaved) await applySaved(lastSaved)
    } catch (error) {
      setConnectError(typeof error === 'string' ? error : 'Could not save preferences.')
    } finally {
      setConnectingId(null)
    }
  }

  const busy = saving || connectingId !== null
  const blocked = !settings

  return (
    <section className="onboarding-card card-enter" aria-labelledby="onboarding-title" data-phase={phase}>
      <div className="onboarding-hero">
        <div className={`radar-stage${phase === 'scanning' ? ' is-scanning' : ' is-idle'}`} aria-hidden="true">
          <span className="radar-ring ring-outer" />
          <span className="radar-ring ring-mid" />
          <span className="radar-ring ring-inner" />
          <span className="radar-sweep" />
          <span className="radar-pulse" />
          <span className="radar-core">
            <img src="./icone-tempoai.png" alt="" width="30" height="30" />
          </span>
        </div>

        <div className="onboarding-wordmark" aria-hidden="true">
          <img className="brand-logo-light" src="./logo-tempoai-light.png" alt="" width="176" height="39" />
          <img className="brand-logo-dark" src="./logo-tempoai-dark.png" alt="" width="176" height="39" />
        </div>

        <div className="onboarding-copy">
          <h2 id="onboarding-title">{t('Connect your first AI tool')}</h2>
          <p>
            {phase === 'scanning'
              ? t('Scanning this computer…')
              : phase === 'results' && result
                ? result.agents.length > 0
                  ? t(
                    result.agents.length === 1
                      ? 'Found 1 AI tool on this computer. Add it with one click.'
                      : 'Found {count} AI tools on this computer. Choose which ones to add.',
                    { count: result.agents.length },
                  )
                  : t('We didn’t find a supported AI tool here. If you just installed one, try searching again.')
                : t('Scan this computer to find Claude Code, OpenAI Codex, Google Antigravity and other local tools. Everything stays on your machine — no scripts or passwords needed.')}
          </p>
          <p className="onboarding-privacy">
            <ShieldCheck aria-hidden="true" />
            <span>{t('Read-only local scan. Conversations and passwords are never saved.')}</span>
          </p>
        </div>

        <div className="onboarding-actions">
          {phase === 'idle' && (
            <>
              <Button onClick={runScan} disabled={saving}>
                <Radar aria-hidden="true" />
                {t('Scan my AI tools')}
              </Button>
              <Button variant="outline" onClick={() => { setShowManualOnly(true); setManualAgent('custom') }} disabled={saving}>
                <FileJson aria-hidden="true" />
                {t('Connect a local usage file')}
              </Button>
            </>
          )}
          {phase === 'scanning' && (
            <Button disabled>
              <RefreshCw className="spin" aria-hidden="true" />
              {t('Scanning this computer…')}
            </Button>
          )}
          {phase === 'results' && (
            <>
              {pendingReady.length > 1 && (
                <Button disabled={busy || blocked} onClick={() => void connectAll()}>
                  <CheckCheck aria-hidden="true" />
                  {connectingId === '__all__'
                    ? t('Connecting tools…')
                    : t('Add all found ({count})', { count: pendingReady.length })}
                </Button>
              )}
              <Button variant="outline" disabled={busy} onClick={runScan}>
                <Radar aria-hidden="true" />
                {t('Scan again')}
              </Button>
              <Button variant="ghost" disabled={busy} onClick={() => setManualAgent((prev) => (prev === 'custom' ? null : 'custom'))}>
                <FileJson aria-hidden="true" />
                {t('Other provider')}
              </Button>
            </>
          )}
        </div>
      </div>

      {scan.isError && (
        <p role="alert" className="form-error onboarding-feedback">{t('Could not scan for providers.')}</p>
      )}
      {connectError && (
        <p role="alert" className="form-error onboarding-feedback">{t(connectError)}</p>
      )}

      {phase === 'results' && result && result.agents.length > 0 && (
        <ul className="detected-grid" aria-label={t('Detected AI tools')}>
          {result.agents.map((agent, index) => {
            const direct = zeroConfigConnection(agent)
            const added = addedIds.includes(agent.id)
            const signals = agentSignals(agent)
            const isManualOpen = manualAgent !== 'custom' && manualAgent?.id === agent.id
            return (
              <li key={agent.id} className="detected-card" style={{ animationDelay: `${Math.min(index, 6) * 40}ms` }}>
                <div className="detected-head">
                  <ProviderMark id={agent.id} />
                  <div className="detected-info">
                    <strong>{providerNames[agent.id]}</strong>
                    <div className="detected-tags">
                      {signals.map((tag) => (
                        <span key={tag} className={`detected-tag${tag === 'Zero-config' ? ' tag-ready' : ''}`}>{t(tag)}</span>
                      ))}
                    </div>
                    {agent.sourcePath && <code className="path-note" title={agent.sourcePath}>{agent.sourcePath}</code>}
                  </div>
                </div>
                <div className="detected-action">
                  {added ? (
                    <span className="detected-added"><Check aria-hidden="true" />{t('Connected')}</span>
                  ) : direct ? (
                    <Button
                      disabled={busy || blocked}
                      onClick={() => void connectOne(agent.id, direct)}
                    >
                      <Plus aria-hidden="true" />
                      {connectingId === agent.id ? t('Saving provider…') : t('Add')}
                    </Button>
                  ) : (
                    <Button
                      variant="outline"
                      disabled={busy || blocked}
                      onClick={() => setManualAgent(isManualOpen ? null : agent)}
                    >
                      <FileJson aria-hidden="true" />
                      {t('Set file')}
                    </Button>
                  )}
                </div>
              </li>
            )
          })}
        </ul>
      )}

      {phase === 'results' && manualAgent && (
        <div className="onboarding-manual body-reveal">
          <LocalProviderForm
            key={manualAgent === 'custom' ? 'custom' : manualAgent.id}
            name={manualAgent === 'custom' ? undefined : providerNames[manualAgent.id]}
            ready={Boolean(settings) && !busy}
            atLimit={(settings?.localProviders?.length ?? 0) >= 20}
            onReview={(name, path) => {
              const id: LocalProvider['id'] = manualAgent === 'custom'
                ? `local-${crypto.randomUUID()}`
                : `local-${manualAgent.id}`
              void connectOne(id, { kind: 'local', source: { id, name, path } })
            }}
          />
        </div>
      )}
    </section>
  )
}
