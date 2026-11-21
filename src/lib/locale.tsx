import { listen } from '@tauri-apps/api/event'
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react'
import { desktop, getUiState } from '@/services/desktop'
import { persistUiPatch } from './display'
import { pt } from './pt'

export type Language = 'en-US' | 'pt-BR'
type Params = Record<string, string | number>
const key = 'tempoai-language'
const interpolate = (message: string, params?: Params) =>
  params ? message.replace(/\{(\w+)\}/g, (match, key: string) => String(params[key] ?? match)) : message

function savedLanguage(): Language {
  try {
    const saved = localStorage.getItem(key)
    if (saved === 'en-US' || saved === 'pt-BR') return saved
  } catch { /* WebView storage may be unavailable. */ }
  return navigator.language.toLowerCase().startsWith('pt') ? 'pt-BR' : 'en-US'
}

type Locale = { language: Language; setLanguage: (language: Language) => void; t: (message: string, params?: Params) => string }
const LocaleContext = createContext<Locale>({ language: 'en-US', setLanguage: () => {}, t: interpolate })

export function LocaleProvider({ children, onLanguage }: { children: ReactNode; onLanguage?: (language: Language) => void }) {
  const [language, setLanguage] = useState(savedLanguage)
  const localVersion = useRef(0)

  useEffect(() => {
    document.documentElement.lang = language
    onLanguage?.(language)
  }, [language, onLanguage])

  useEffect(() => {
    const sync = (event: StorageEvent) => {
      if (event.key === key) setLanguage(savedLanguage())
    }
    window.addEventListener('storage', sync)
    return () => window.removeEventListener('storage', sync)
  }, [])

  useEffect(() => {
    if (!desktop || typeof getUiState !== 'function') return
    let active = true
    const pull = () => {
      const started = localVersion.current
      const pending = getUiState()
      if (!pending || typeof pending.then !== 'function') return
      void pending.then((remote) => {
        if (!active || localVersion.current !== started) return
        if (remote?.language === 'en-US' || remote?.language === 'pt-BR') {
          setLanguage(remote.language)
          try { localStorage.setItem(key, remote.language) } catch { /* ignore */ }
        }
      }).catch(() => {})
    }
    pull()
    const stop = listen('ui-state-updated', () => {
      localVersion.current += 1
      pull()
    })
    void stop.catch(() => {})
    return () => {
      active = false
      void stop.then((unlisten) => unlisten()).catch(() => {})
    }
  }, [])

  const changeLanguage = (next: Language) => {
    localVersion.current += 1
    setLanguage(next)
    try { localStorage.setItem(key, next) } catch { /* The current window still switches language. */ }
    persistUiPatch({ language: next })
  }

  const t = (message: string, params?: Params) => {
    if (language !== 'pt-BR') return interpolate(message, params)
    const backup = /^Preferences were damaged\. Defaults are in use and the original was saved as (preferences\.json\.bak(?:\.\d+)?)\.$/.exec(message)
    if (backup) return interpolate(pt['Preferences were damaged. Defaults are in use and the original was saved as {file}.'], { file: backup[1] })
    return interpolate(Object.hasOwn(pt, message) ? pt[message] : message, params)
  }
  return <LocaleContext.Provider value={{ language, setLanguage: changeLanguage, t }}>{children}</LocaleContext.Provider>
}

export const useLocale = () => useContext(LocaleContext)
