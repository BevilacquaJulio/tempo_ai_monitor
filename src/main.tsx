import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { HashRouter } from 'react-router-dom'
import '@fontsource-variable/geist/wght.css'
import './styles/index.css'
import { App } from './app/app'
import { desktop, quickWindow, setDesktopLanguage, setWindowTheme } from './services/desktop'
import { QuickMonitor } from './features/quick-monitor/quick-monitor'
import { LocaleProvider, type Language } from './lib/locale'
import { applyFontScale, applyTheme, DisplayProvider, savedFontScale, savedTheme, type Theme } from './lib/display'

const queryClient = new QueryClient()
const root = document.getElementById('root')
const syncDesktopLanguage = desktop && !quickWindow
  ? (language: Language) => { void setDesktopLanguage(language === 'pt-BR').catch(() => {}) }
  : undefined
// Keeps the Windows title bar in the same theme as the page.
const syncWindowTheme = desktop
  ? (theme: Theme) => { void setWindowTheme(theme === 'system' ? null : theme).catch(() => {}) }
  : undefined

if (!root) throw new Error('TempoAI root element is missing')
// Applied before the first paint so saved theme and font scale never flash defaults.
applyTheme(savedTheme())
applyFontScale(savedFontScale())

createRoot(root).render(<StrictMode>
  <QueryClientProvider client={queryClient}>
    <LocaleProvider onLanguage={syncDesktopLanguage}>
      <DisplayProvider onTheme={syncWindowTheme}>{quickWindow ? <QuickMonitor /> : <HashRouter><App /></HashRouter>}</DisplayProvider>
    </LocaleProvider>
  </QueryClientProvider>
</StrictMode>)
