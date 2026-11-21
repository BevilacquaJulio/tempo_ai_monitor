import { listen } from '@tauri-apps/api/event'
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react'
import { desktop, getUiState, saveUiState, type UiState, type UiStatePatch } from '@/services/desktop'

export type Theme = 'system' | 'light' | 'dark'
export type PercentStyle = 'remaining' | 'used'
export type AppRoute = '/' | '/activity' | '/settings'
export type ActivityPeriod = 7 | 30 | 90
export type HeatmapRange = 30 | 90

export const MIN_FONT_SCALE = 85
export const MAX_FONT_SCALE = 125
export const DEFAULT_FONT_SCALE = 100
export const FONT_SCALE_STEP = 5

const themeKey = 'tempoai-theme'
const fontScaleKey = 'tempoai-font-scale'
const percentKey = 'tempoai-percent-style'
const perToolKey = 'tempoai-percent-by-tool'
const collapsedOverviewKey = 'tempoai-collapsed-overview'
const collapsedQuickKey = 'tempoai-collapsed-quick'
const lastRouteKey = 'tempoai-last-route'
const activityPeriodKey = 'tempoai-activity-period'
const heatmapDaysKey = 'tempoai-heatmap-days'
const dismissedGuidanceKey = 'tempoai-dismissed-guidance'

function read<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const saved = localStorage.getItem(key)
    if (saved && (allowed as readonly string[]).includes(saved)) return saved as T
  } catch { /* WebView storage may be unavailable. */ }
  return fallback
}

export function readJson<T>(key: string, valid: (value: unknown) => value is T, fallback: T): T {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? 'null')
    if (valid(value)) return value
  } catch { /* Missing or damaged values fall back to the default. */ }
  return fallback
}

export function store(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key)
    else localStorage.setItem(key, value)
  } catch { /* This window still updates. */ }
}

const isStyleMap = (value: unknown): value is Record<string, PercentStyle> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
  && Object.values(value).every((style) => style === 'remaining' || style === 'used')

const isStringList = (value: unknown): value is string[] =>
  Array.isArray(value) && value.every((item) => typeof item === 'string')

const isText = (value: unknown): value is string => typeof value === 'string'

export function clampFontScale(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_FONT_SCALE
  const clamped = Math.min(MAX_FONT_SCALE, Math.max(MIN_FONT_SCALE, value))
  return Math.round(clamped / FONT_SCALE_STEP) * FONT_SCALE_STEP
}

export const savedTheme = () => read<Theme>(themeKey, ['system', 'light', 'dark'], 'system')
export const savedFontScale = () => {
  try {
    const raw = localStorage.getItem(fontScaleKey)
    if (raw !== null) {
      const parsed = Number(raw)
      if (Number.isFinite(parsed)) return clampFontScale(parsed)
    }
  } catch { /* WebView storage may be unavailable. */ }
  return DEFAULT_FONT_SCALE
}
const savedPercentStyle = () => read<PercentStyle>(percentKey, ['remaining', 'used'], 'remaining')
const savedPerTool = () => readJson(perToolKey, isStyleMap, {})
const savedCollapsed = (place: 'overview' | 'quick') => readJson(place === 'overview' ? collapsedOverviewKey : collapsedQuickKey, isStringList, [])
const savedLastRoute = () => read<AppRoute>(lastRouteKey, ['/', '/activity', '/settings'], '/')
const savedActivityPeriod = (): ActivityPeriod => {
  const raw = Number(read(activityPeriodKey, ['7', '30', '90'], '7'))
  return raw === 30 || raw === 90 ? raw : 7
}
const savedHeatmapDays = (): HeatmapRange => {
  const raw = Number(read(heatmapDaysKey, ['30', '90'], '90'))
  return raw === 30 ? 30 : 90
}
const savedDismissedGuidance = () => readJson(dismissedGuidanceKey, isText, '')

/** Sends only the edited fields to Rust so concurrent writes across Main and Quick Monitor never clobber each other. */
export function persistUiPatch(patch: UiStatePatch) {
  if (!desktop || typeof saveUiState !== 'function') return
  const pending = saveUiState(patch)
  if (pending && typeof pending.catch === 'function') void pending.catch(() => {})
}

export function applyTheme(theme: Theme) {
  const root = document.documentElement
  if (theme === 'system') root.removeAttribute('data-theme')
  else root.setAttribute('data-theme', theme)
}

export function applyFontScale(scale: number) {
  const clamped = clampFontScale(scale)
  document.documentElement.style.setProperty('--font-scale', String(clamped / 100))
}

type Display = {
  theme: Theme
  fontScale: number
  /** Default for every tool, chosen in Settings. */
  percentStyle: PercentStyle
  setTheme: (theme: Theme) => void
  setFontScale: (scale: number) => void
  setPercentStyle: (style: PercentStyle) => void
  /** What one tool shows: its own choice from its card, or the default. */
  styleFor: (toolId: string) => PercentStyle
  toggleStyleFor: (toolId: string) => void
  collapsedOverview: string[]
  collapsedQuick: string[]
  toggleCollapsed: (place: 'overview' | 'quick', toolId: string) => void
  lastRoute: AppRoute
  setLastRoute: (route: AppRoute) => void
  activityPeriod: ActivityPeriod
  setActivityPeriod: (period: ActivityPeriod) => void
  heatmapDays: HeatmapRange
  setHeatmapDays: (days: HeatmapRange) => void
  dismissedGuidance: string
  setDismissedGuidance: (identity: string) => void
}

const DisplayContext = createContext<Display>({
  theme: 'system',
  fontScale: DEFAULT_FONT_SCALE,
  percentStyle: 'remaining',
  setTheme: () => {},
  setFontScale: () => {},
  setPercentStyle: () => {},
  styleFor: () => 'remaining',
  toggleStyleFor: () => {},
  collapsedOverview: [],
  collapsedQuick: [],
  toggleCollapsed: () => {},
  lastRoute: '/',
  setLastRoute: () => {},
  activityPeriod: 7,
  setActivityPeriod: () => {},
  heatmapDays: 90,
  setHeatmapDays: () => {},
  dismissedGuidance: '',
  setDismissedGuidance: () => {},
})

export function DisplayProvider({ children, onTheme }: { children: ReactNode; onTheme?: (theme: Theme) => void }) {
  const [theme, setThemeState] = useState(savedTheme)
  const [fontScale, setFontScaleState] = useState(savedFontScale)
  const [percentStyle, setPercentState] = useState(savedPercentStyle)
  const [perTool, setPerTool] = useState(savedPerTool)
  const [collapsedOverview, setCollapsedOverview] = useState(() => savedCollapsed('overview'))
  const [collapsedQuick, setCollapsedQuick] = useState(() => savedCollapsed('quick'))
  const [lastRoute, setLastRouteState] = useState(savedLastRoute)
  const [activityPeriod, setActivityPeriodState] = useState(savedActivityPeriod)
  const [heatmapDays, setHeatmapDaysState] = useState(savedHeatmapDays)
  const [dismissedGuidance, setDismissedGuidanceState] = useState(savedDismissedGuidance)
  const localVersion = useRef(0)

  const hydrateFromRemote = useCallback((remote: UiState | undefined | null) => {
    if (!remote || typeof remote !== 'object') return
    if (remote.theme === 'system' || remote.theme === 'light' || remote.theme === 'dark') {
      setThemeState(remote.theme)
      store(themeKey, remote.theme)
    }
    if (typeof remote.fontScale === 'number' && Number.isFinite(remote.fontScale)) {
      const clamped = clampFontScale(remote.fontScale)
      setFontScaleState(clamped)
      store(fontScaleKey, String(clamped))
    }
    if (remote.percentStyle === 'remaining' || remote.percentStyle === 'used') {
      setPercentState(remote.percentStyle)
      store(percentKey, remote.percentStyle)
    }
    if (isStyleMap(remote.perToolPercent)) {
      setPerTool(remote.perToolPercent)
      store(perToolKey, JSON.stringify(remote.perToolPercent))
    }
    if (isStringList(remote.collapsedOverview)) {
      setCollapsedOverview(remote.collapsedOverview)
      store(collapsedOverviewKey, JSON.stringify(remote.collapsedOverview))
    }
    if (isStringList(remote.collapsedQuick)) {
      setCollapsedQuick(remote.collapsedQuick)
      store(collapsedQuickKey, JSON.stringify(remote.collapsedQuick))
    }
    if (remote.lastRoute === '/' || remote.lastRoute === '/activity' || remote.lastRoute === '/settings') {
      setLastRouteState(remote.lastRoute)
      store(lastRouteKey, remote.lastRoute)
    }
    if (remote.activityPeriod === 7 || remote.activityPeriod === 30 || remote.activityPeriod === 90) {
      setActivityPeriodState(remote.activityPeriod)
      store(activityPeriodKey, String(remote.activityPeriod))
    }
    if (remote.heatmapDays === 30 || remote.heatmapDays === 90) {
      setHeatmapDaysState(remote.heatmapDays)
      store(heatmapDaysKey, String(remote.heatmapDays))
    }
    if (typeof remote.dismissedGuidance === 'string') {
      setDismissedGuidanceState(remote.dismissedGuidance)
      store(dismissedGuidanceKey, JSON.stringify(remote.dismissedGuidance))
    }
  }, [])

  useEffect(() => { applyTheme(theme); onTheme?.(theme) }, [theme, onTheme])
  useEffect(() => { applyFontScale(fontScale) }, [fontScale])

  useEffect(() => {
    const sync = (event: StorageEvent) => {
      if (event.key === themeKey) setThemeState(savedTheme())
      if (event.key === fontScaleKey) setFontScaleState(savedFontScale())
      if (event.key === percentKey) setPercentState(savedPercentStyle())
      if (event.key === perToolKey) setPerTool(savedPerTool())
      if (event.key === collapsedOverviewKey) setCollapsedOverview(savedCollapsed('overview'))
      if (event.key === collapsedQuickKey) setCollapsedQuick(savedCollapsed('quick'))
      if (event.key === lastRouteKey) setLastRouteState(savedLastRoute())
      if (event.key === activityPeriodKey) setActivityPeriodState(savedActivityPeriod())
      if (event.key === heatmapDaysKey) setHeatmapDaysState(savedHeatmapDays())
      if (event.key === dismissedGuidanceKey) setDismissedGuidanceState(savedDismissedGuidance())
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
        if (active && localVersion.current === started) hydrateFromRemote(remote)
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
  }, [hydrateFromRemote])

  const styleFor = (toolId: string) => perTool[toolId] ?? percentStyle

  const commit = (patch: UiStatePatch) => {
    localVersion.current += 1
    persistUiPatch(patch)
  }

  return <DisplayContext.Provider value={{
    theme,
    fontScale,
    percentStyle,
    styleFor,
    setTheme: (next) => {
      setThemeState(next)
      store(themeKey, next)
      commit({ theme: next })
    },
    setFontScale: (next) => {
      const clamped = clampFontScale(next)
      setFontScaleState(clamped)
      store(fontScaleKey, String(clamped))
      commit({ fontScale: clamped })
    },
    setPercentStyle: (next) => {
      setPercentState(next)
      setPerTool({})
      store(percentKey, next)
      store(perToolKey, JSON.stringify({}))
      commit({ percentStyle: next, perToolPercent: {} })
    },
    toggleStyleFor: (toolId) => {
      const next: PercentStyle = styleFor(toolId) === 'remaining' ? 'used' : 'remaining'
      const override = next === percentStyle ? null : next
      const others = Object.fromEntries(Object.entries(perTool).filter(([id]) => id !== toolId)) as Record<string, PercentStyle>
      const updated: Record<string, PercentStyle> = override === null ? others : { ...others, [toolId]: override }
      setPerTool(updated)
      store(perToolKey, JSON.stringify(updated))
      commit({ perToolOverride: [toolId, override] })
    },
    collapsedOverview,
    collapsedQuick,
    toggleCollapsed: (place, toolId) => {
      const list = place === 'overview' ? collapsedOverview : collapsedQuick
      const next = list.includes(toolId) ? list.filter((id) => id !== toolId) : [...list, toolId]
      if (place === 'overview') {
        setCollapsedOverview(next)
        store(collapsedOverviewKey, JSON.stringify(next))
        commit({ collapsedOverview: next })
      } else {
        setCollapsedQuick(next)
        store(collapsedQuickKey, JSON.stringify(next))
        commit({ collapsedQuick: next })
      }
    },
    lastRoute,
    setLastRoute: (route) => {
      setLastRouteState(route)
      store(lastRouteKey, route)
      commit({ lastRoute: route })
    },
    activityPeriod,
    setActivityPeriod: (period) => {
      setActivityPeriodState(period)
      store(activityPeriodKey, String(period))
      commit({ activityPeriod: period })
    },
    heatmapDays,
    setHeatmapDays: (days) => {
      setHeatmapDaysState(days)
      store(heatmapDaysKey, String(days))
      commit({ heatmapDays: days })
    },
    dismissedGuidance,
    setDismissedGuidance: (identity) => {
      setDismissedGuidanceState(identity)
      store(dismissedGuidanceKey, JSON.stringify(identity))
      commit({ dismissedGuidance: identity })
    },
  }}>{children}</DisplayContext.Provider>
}

export const useDisplay = () => useContext(DisplayContext)

/** The share shown to people: what is left or what was used, depending on their choice. */
export function shownShare(used: number, style: PercentStyle) {
  return style === 'remaining' ? 100 - used : used
}

/** Which tools are collapsed in one place (the dashboard or Quick Monitor); persisted to preferences.json. */
export function useCollapsedTools(place: 'overview' | 'quick') {
  const { collapsedOverview, collapsedQuick, toggleCollapsed } = useDisplay()
  const list = place === 'overview' ? collapsedOverview : collapsedQuick
  return {
    isCollapsed: (toolId: string) => list.includes(toolId),
    toggle: (toolId: string) => toggleCollapsed(place, toolId),
  }
}
