import { Check, Copy, Download, Share2, ShieldCheck, X } from 'lucide-react'
import { Dialog, Switch } from 'radix-ui'
import { useMemo, useState } from 'react'
import { Button } from '@/components/ui/button'
import { useDisplay } from '@/lib/display'
import { useLocale } from '@/lib/locale'
import type { ProviderId, SessionRecord } from '@/providers/types'
import { filterSessions, type Period } from './activity-data'
import { buildShareMarkdown, buildShareSvg } from './share-card'

export function ShareStudio({ sessions, providers, now, defaultPeriod }: {
  sessions: SessionRecord[]
  providers: [string, string][]
  now: number
  defaultPeriod: Period
}) {
  const { t, language } = useLocale()
  const { theme: appTheme } = useDisplay()
  const [open, setOpen] = useState(false)
  const [period, setPeriod] = useState<Period>(defaultPeriod)
  const [provider, setProvider] = useState<ProviderId | ''>('')
  const [cardTheme, setCardTheme] = useState<'dark' | 'light'>(() => (appTheme === 'light' ? 'light' : 'dark'))
  const [hidePrivateNames, setHidePrivateNames] = useState(true)
  const [copied, setCopied] = useState<'svg' | 'md' | null>(null)

  const scoped = useMemo(
    () => filterSessions(sessions, { period, provider, model: '', project: '', branch: '' }, now),
    [sessions, period, provider, now],
  )
  const options = useMemo(
    () => ({ period, provider, theme: cardTheme, hidePrivateNames, language, t }),
    [period, provider, cardTheme, hidePrivateNames, language, t],
  )
  const svg = useMemo(() => buildShareSvg(scoped, options), [scoped, options])
  const markdown = useMemo(() => buildShareMarkdown(scoped, options), [scoped, options])
  const previewSrc = useMemo(() => `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`, [svg])

  const copyText = async (kind: 'svg' | 'md', text: string) => {
    try {
      await navigator.clipboard?.writeText(text)
      setCopied(kind)
      window.setTimeout(() => setCopied((prev) => (prev === kind ? null : prev)), 2200)
    } catch {
      setCopied(null)
    }
  }

  const downloadSvg = () => {
    const blob = new Blob([svg], { type: 'image/svg+xml;charset=utf-8' })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = `tempoai-summary-${period}d.svg`
    link.click()
    URL.revokeObjectURL(url)
  }

  return (
    <Dialog.Root open={open} onOpenChange={(next) => { setOpen(next); if (next) { setPeriod(defaultPeriod); setCopied(null) } }}>
      <Dialog.Trigger asChild>
        <Button variant="outline">
          <Share2 aria-hidden="true" />
          {t('Share summary')}
        </Button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="drawer-overlay" />
        <Dialog.Content className="confirm-dialog share-dialog">
          <header className="share-header">
            <div>
              <Dialog.Title>{t('Share Studio')}</Dialog.Title>
              <Dialog.Description>{t('Preview and export a privacy-safe local summary card. Nothing is uploaded.')}</Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <Button variant="ghost" size="icon" aria-label={t('Close')} title={t('Close')}><X aria-hidden="true" /></Button>
            </Dialog.Close>
          </header>

          <div className="share-controls">
            <div className="segments" role="group" aria-label={t('Period')}>
              {([7, 30, 90] as Period[]).map((p) => (
                <Button key={p} variant="ghost" aria-pressed={period === p} onClick={() => setPeriod(p)}>
                  {t('{count} days', { count: p })}
                </Button>
              ))}
            </div>
            <div className="segments" role="group" aria-label={t('Card theme')}>
              {(['dark', 'light'] as const).map((mode) => (
                <Button key={mode} variant="ghost" aria-pressed={cardTheme === mode} onClick={() => setCardTheme(mode)}>
                  {t(mode === 'dark' ? 'Dark' : 'Light')}
                </Button>
              ))}
            </div>
            <label className="share-select">
              <span className="sr-only">{t('Provider')}</span>
              <select value={provider} onChange={(event) => setProvider(event.target.value as ProviderId | '')}>
                <option value="">{t('All providers')}</option>
                {providers.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
              </select>
            </label>
          </div>

          <div className="share-privacy-row">
            <label htmlFor="share-hide-private" className="share-privacy-label">
              <ShieldCheck aria-hidden="true" />
              <span>{t('Hide project & branch names')}</span>
            </label>
            <Switch.Root id="share-hide-private" className="switch" checked={hidePrivateNames} onCheckedChange={setHidePrivateNames}>
              <Switch.Thumb />
            </Switch.Root>
          </div>

          <div className="share-preview">
            <img src={previewSrc} alt={t('Usage summary card preview')} width="600" height="352" />
          </div>

          <div className="share-actions">
            <Button onClick={downloadSvg}>
              <Download aria-hidden="true" />
              {t('Download SVG')}
            </Button>
            <Button variant="outline" onClick={() => void copyText('svg', svg)}>
              {copied === 'svg' ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
              {t(copied === 'svg' ? 'Copied' : 'Copy SVG')}
            </Button>
            <Button variant="outline" onClick={() => void copyText('md', markdown)}>
              {copied === 'md' ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
              {t(copied === 'md' ? 'Copied' : 'Copy Markdown')}
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
