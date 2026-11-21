import { Activity as ActivityIcon, FlaskConical, LayoutGrid, Settings as SettingsIcon } from 'lucide-react'
import { useEffect, useEffectEvent, useRef } from 'react'
import { NavLink, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router-dom'
import { listen } from '@tauri-apps/api/event'
import { Activity } from '@/features/activity/activity'
import { Overview } from '@/features/overview/overview'
import { Settings } from '@/features/settings/settings'
import type { UsageReader } from '@/providers/types'
import { readUsage } from '@/services/usage'
import { desktop, getPreferences } from '@/services/desktop'
import { useUsage } from '@/hooks/use-usage'
import { useQuery } from '@tanstack/react-query'
import { DesktopSettings } from '@/features/settings/desktop-settings'
import { usePreferencesDraft } from '@/features/settings/use-preferences-draft'
import { ProviderScan } from '@/features/overview/provider-scan'
import { useDisplay, type AppRoute } from '@/lib/display'
import { useLocale } from '@/lib/locale'

const navigation = [
  { href: '/', label: 'Overview', icon: LayoutGrid },
  { href: '/activity', label: 'Activity', icon: ActivityIcon },
  { href: '/settings', label: 'Settings', icon: SettingsIcon },
]

export function App({ reader = readUsage, development = import.meta.env.DEV && !desktop }: { reader?: UsageReader; development?: boolean }) {
  const { t } = useLocale()
  const { lastRoute, setLastRoute } = useDisplay()
  const { query, refresh } = useUsage(reader)
  const preferences = useQuery({ queryKey: ['preferences'], queryFn: getPreferences, enabled: desktop, retry: false, refetchOnWindowFocus: 'always' })
  const draft = usePreferencesDraft(refresh)
  const demo = query.data ? query.data.mode === 'demo' : development
  const location = useLocation()
  const navigate = useNavigate()
  const openSettings = useEffectEvent(() => { void navigate('/settings') })
  const refetchPreferences = useEffectEvent(() => { void preferences.refetch() })
  const previous = useRef(location.pathname)
  const restoredRoute = useRef(false)
  const main = useRef<HTMLElement>(null)
  const page = t(navigation.find(({ href }) => href === location.pathname)?.label ?? 'Overview')

  // Restore the last active dashboard route on cold startup when opening at the default root.
  useEffect(() => {
    if (restoredRoute.current) return
    if (location.pathname !== '/') {
      restoredRoute.current = true
      return
    }
    if (desktop && lastRoute !== '/') {
      restoredRoute.current = true
      void navigate(lastRoute, { replace: true })
    }
  }, [lastRoute, location.pathname, navigate])

  useEffect(() => {
    document.title = `${page} · TempoAI`
    if (previous.current === location.pathname) return
    previous.current = location.pathname
    restoredRoute.current = true
    if (location.pathname === '/' || location.pathname === '/activity' || location.pathname === '/settings') {
      setLastRoute(location.pathname as AppRoute)
    }
    main.current?.scrollTo?.(0, 0)
    main.current?.querySelector('h1')?.focus({ preventScroll: true })
  }, [location.pathname, page, setLastRoute])

  useEffect(() => {
    if (!desktop) return
    const stopSettings = listen('open-settings', () => openSettings())
    const stopPrefs = listen('preferences-updated', () => refetchPreferences())
    void stopSettings.catch(() => {})
    void stopPrefs.catch(() => {})
    return () => {
      void stopSettings.then((unlisten) => unlisten()).catch(() => {})
      void stopPrefs.then((unlisten) => unlisten()).catch(() => {})
    }
  }, [])

  const scan = <ProviderScan settings={preferences.data?.settings} saving={draft.save.isPending} />

  return <div className="app-shell">
    <a className="skip-link" href="#main-content" onClick={(event) => { event.preventDefault(); main.current?.focus() }}>{t('Skip to content')}</a>
    <aside className="sidebar">
      <div className="brand">
        <img className="brand-wordmark brand-logo-light" src="./logo-tempoai-light.png" alt="" width="152" height="33" />
        <img className="brand-wordmark brand-logo-dark" src="./logo-tempoai-dark.png" alt="" width="152" height="33" />
        <img className="brand-icon-compact" src="./icone-tempoai-fundo.png" alt="" width="32" height="32" />
        <strong className="sr-only">TempoAI</strong>
      </div>
      <nav aria-label={t('Workspace')}>{navigation.map(({ href, label, icon: Icon }) => <NavLink key={href} to={href} end title={t(label)}>
        <Icon aria-hidden="true" strokeWidth={1.75} /><span>{t(label)}</span>
      </NavLink>)}</nav>
      {demo && <p className="sidebar-demo"><FlaskConical aria-hidden="true" /><span>{t('Demo workspace')}</span></p>}
    </aside>
    <div className="workspace">
      <main id="main-content" ref={main} tabIndex={-1}><div key={location.pathname} className="page page-enter">
        <Routes>
          <Route path="/" element={<Overview query={query} demo={demo} refresh={refresh} scan={scan}
            settings={preferences.data?.settings} saving={draft.save.isPending} />} />
          <Route path="/activity" element={<Activity query={query} demo={demo} refresh={refresh} />} />
          <Route path="/settings" element={desktop
            ? <DesktopSettings preferences={preferences} editor={draft} data={query.data} reloadUsage={query.refetch} scan={scan} />
            : <Settings data={query.data} demo={demo} />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </div></main>
    </div>
  </div>
}
