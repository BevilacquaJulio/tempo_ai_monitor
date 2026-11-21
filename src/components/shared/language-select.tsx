import { useLocale } from '@/lib/locale'

export function LanguageSelect() {
  const { language, setLanguage, t } = useLocale()
  return <select className="language-select" id="interface-language" aria-label={t('Interface language')} value={language}
    onChange={(event) => setLanguage(event.target.value === 'pt-BR' ? 'pt-BR' : 'en-US')}>
    <option value="en-US" lang="en">English</option>
    <option value="pt-BR" lang="pt-BR">Português (Brasil)</option>
  </select>
}
