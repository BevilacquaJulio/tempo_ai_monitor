import { Minus, Monitor, Moon, Plus, Sun, type LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { PageHeading } from '@/components/shared/page-heading'
import { ProviderMark } from '@/components/providers/provider-mark'
import { StatusIndicator } from '@/components/shared/states'
import { Button } from '@/components/ui/button'
import type { UsageSnapshot } from '@/providers/types'
import { DEFAULT_FONT_SCALE, FONT_SCALE_STEP, MAX_FONT_SCALE, MIN_FONT_SCALE, useDisplay, type PercentStyle, type Theme } from '@/lib/display'
import { formatPercent } from '@/lib/format'
import { useLocale } from '@/lib/locale'
import { LanguageSelect } from '@/components/shared/language-select'

type SettingsProps = { data?: UsageSnapshot; demo: boolean }

/** Browser preview of Settings. The Windows app uses DesktopSettings, which saves to the local preferences file. */
export function Settings({ data, demo }: SettingsProps) {
  const { t } = useLocale()
  return <>
    <PageHeading title="Settings" description="Choose how TempoAI looks and behaves." />
    <div className="settings-stack">
      <AppearanceSection />
      <SettingsSection title="Your AI tools" description="The tools TempoAI is keeping an eye on.">
        {data?.providers.map((provider) => <div className="tool-row" key={provider.providerId}>
          <ProviderMark id={provider.providerId} />
          <div className="tool-name"><strong>{provider.displayName}</strong><span>{t(demo ? 'Example data' : 'Not connected')}</span></div>
          <StatusIndicator label={provider.integration === 'demo' ? 'Sample data' : 'Not connected'} tone={provider.integration === 'demo' ? 'accent' : 'neutral'} />
        </div>)}
        <p className="settings-note">{t('Open the TempoAI Windows app to connect your AI tools.')}</p>
      </SettingsSection>
    </div>
  </>
}

const themes: { value: Theme; label: string; icon: LucideIcon }[] = [
  { value: 'system', label: 'Same as Windows', icon: Monitor },
  { value: 'light', label: 'Light', icon: Sun },
  { value: 'dark', label: 'Dark', icon: Moon },
]

/** Look and language apply right away and are persisted automatically on this computer. */
export function AppearanceSection() {
  const { t, language } = useLocale()
  const { theme, setTheme, fontScale, setFontScale, percentStyle, setPercentStyle } = useDisplay()
  const sample = (style: PercentStyle) => t(style === 'remaining' ? '{value}% left' : '{value}% used', { value: formatPercent(style === 'remaining' ? 73 : 27, language) })
  return <SettingsSection title="Appearance" description="Changes here apply right away and are remembered automatically.">
    <SettingRow title="Theme" value={<ChoiceGroup label="Theme" value={theme} onChange={setTheme} options={themes} />} />
    <SettingRow title="Font size" description="Applies to the dashboard and Quick Monitor." value={
      <div className="font-scale-control" role="group" aria-label={t('Font size')}>
        <Button type="button" variant="outline" size="icon"
          aria-label={t('Decrease font size')} title={t('Decrease font size')}
          disabled={fontScale <= MIN_FONT_SCALE}
          onClick={() => setFontScale(fontScale - FONT_SCALE_STEP)}>
          <Minus aria-hidden="true" />
        </Button>
        <button type="button" className="font-scale-readout"
          aria-label={t('Reset font size to {value}%', { value: DEFAULT_FONT_SCALE })}
          title={fontScale === DEFAULT_FONT_SCALE ? t('Default size') : t('Reset font size to {value}%', { value: DEFAULT_FONT_SCALE })}
          disabled={fontScale === DEFAULT_FONT_SCALE}
          onClick={() => setFontScale(DEFAULT_FONT_SCALE)}>
          <span key={fontScale} className="figure-value">{fontScale}%</span>
        </button>
        <Button type="button" variant="outline" size="icon"
          aria-label={t('Increase font size')} title={t('Increase font size')}
          disabled={fontScale >= MAX_FONT_SCALE}
          onClick={() => setFontScale(fontScale + FONT_SCALE_STEP)}>
          <Plus aria-hidden="true" />
        </Button>
      </div>
    } />
    <SettingRow title="Show percentages as" description="Pick what is easier for you to read." value={
      <ChoiceGroup label="Show percentages as" value={percentStyle} onChange={setPercentStyle} options={[
        { value: 'remaining', label: sample('remaining') }, { value: 'used', label: sample('used') },
      ]} />
    } />
    <SettingRow title="Interface language" controlId="interface-language" value={<LanguageSelect />} />
  </SettingsSection>
}

export function ChoiceGroup<T extends string>({ label, value, onChange, options }: {
  label: string; value: T; onChange: (value: T) => void; options: { value: T; label: string; icon?: LucideIcon }[]
}) {
  const { t } = useLocale()
  return <div className="choice-group" role="radiogroup" aria-label={t(label)}>
    {options.map(({ value: option, label: text, icon: Icon }) => <label key={option} className="choice">
      <input type="radio" name={label} value={option} checked={option === value} onChange={() => onChange(option)} />
      {Icon && <Icon aria-hidden="true" />}<span>{t(text)}</span>
    </label>)}
  </div>
}

export function SettingsSection({ title, description, children, id }: { title: string; description?: string; children: ReactNode; id?: string }) {
  const { t } = useLocale()
  const heading = `${id ?? title.toLowerCase().replace(/\W+/g, '-')}-heading`
  return <section className="settings-section card-enter" aria-labelledby={heading}>
    <header><h2 id={heading}>{t(title)}</h2>{description && <p>{t(description)}</p>}</header>
    <div className="settings-card">{children}</div>
  </section>
}

export function SettingRow({ title, value, description, controlId }: { title: string; value: ReactNode; description?: string; controlId?: string }) {
  const { t } = useLocale()
  return <div className="setting-row">
    <div className="setting-label">{controlId ? <label htmlFor={controlId}>{t(title)}</label> : <span>{t(title)}</span>}{description && <p>{t(description)}</p>}</div>
    <div className="setting-value">{typeof value === 'string' ? t(value) : value}</div>
  </div>
}
