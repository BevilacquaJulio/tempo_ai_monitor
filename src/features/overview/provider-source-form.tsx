import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/locale'
import { isUsageFilePath } from '@/providers/local-source'

const example = JSON.stringify({
  observedAt: '<RFC3339>',
  fiveHour: { usedPercent: 42 },
  weekly: { usedPercent: 18, resetAt: '<RFC3339>' },
}, null, 2)

export function LocalProviderForm({ name, path, ready, atLimit, onReview }: {
  name?: string; path?: string; ready: boolean; atLimit: boolean; onReview: (name: string, path: string) => void
}) {
  const { t } = useLocale()
  const [label, setLabel] = useState(name ?? '')
  const [file, setFile] = useState(path ?? '')
  const validPath = isUsageFilePath(file)
  return <form className="scan-setup body-reveal" onSubmit={(event) => {
    event.preventDefault()
    if (ready && !atLimit && label.trim() && validPath) onReview(label.trim(), file.trim())
  }}>
    <h3>{t('Connect a local usage file')}</h3>
    <p>{t('This agent has no native collector in TempoAI. Connect a JSON file populated by your own exporter, or update it manually.')}</p>
    <label className="text-field scan-field">{t('Provider name')}<input required maxLength={80} value={label} onChange={(event) => setLabel(event.target.value)} /></label>
    <label className="text-field scan-field">{t('Path to the local usage JSON file')}<input required maxLength={1024} value={file}
      onChange={(event) => setFile(event.target.value)} placeholder={'C:\\Usage\\agent.json'}
      aria-invalid={file && !validPath ? true : undefined} aria-describedby="usage-file-hint" /></label>
    <p id="usage-file-hint">{t('Use the full path to a .json file on a local drive.')}</p>
    <details>
      <summary>{t('Show usage file format')}</summary>
      <p>{t('Include the observation time with its time zone and at least one quota. The renewal time is optional. TempoAI marks readings older than five minutes as stale.')}</p>
      <pre tabIndex={0}>{example}</pre>
    </details>
    {atLimit && <p>{t('You can connect up to 20 local usage files.')}</p>}
    <Button type="submit" disabled={!ready || atLimit || !label.trim() || !validPath}>{t('Connect provider')}</Button>
  </form>
}
