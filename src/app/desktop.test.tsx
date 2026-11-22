import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { QuickMonitor } from '@/features/quick-monitor/quick-monitor'
import { readDemoUsage } from '@/services/demo-usage'
import { DisplayProvider } from '@/lib/display'
import { LocaleProvider } from '@/lib/locale'
import { App } from './app'

const desktop = vi.hoisted(() => ({
  getPreferences: vi.fn(), savePreferences: vi.fn(), testSavedIntegrations: vi.fn(),
  getDataLocation: vi.fn(), hidePanel: vi.fn(), readDesktopUsage: vi.fn(), readDesktopSessions: vi.fn(),
  refreshDesktop: vi.fn(), refreshClaude: vi.fn(), refreshAntigravity: vi.fn(), setPanelPin: vi.fn(), scanProviders: vi.fn(),
  connectProvider: vi.fn(), removeProvider: vi.fn(), getUiState: vi.fn(), saveUiState: vi.fn(),
  checkForUpdates: vi.fn(), openReleaseUrl: vi.fn(),
}))
const events = vi.hoisted(() => new Map<string, () => void>())
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, callback: () => void) => {
    events.set(name, callback)
    return () => events.delete(name)
  },
}))
vi.mock('@/services/desktop', () => ({ ...desktop, desktop: true, clearHistory: vi.fn(), openDashboard: vi.fn() }))

const saved = { settings: {
  autostart: false, minimizeToTray: true, openPanelAtStartup: false, pinPanel: false,
  shortcut: 'Ctrl+Alt+W', claudeEnabled: false, codexEnabled: false, antigravityEnabled: false,
  notifications: false, claudeAlerts: true, codexAlerts: true, antigravityAlerts: true, thresholds: [80, 90, 95, 100],
}, shortcutConflict: false, recoveryWarning: null }

const defaultUi = {
  language: null, theme: 'system', fontScale: 100, percentStyle: 'remaining', perToolPercent: {},
  collapsedOverview: [], collapsedQuick: [], lastRoute: '/', activityPeriod: 7, heatmapDays: 90, dismissedGuidance: '',
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  desktop.getPreferences.mockResolvedValue(saved)
  desktop.getUiState.mockResolvedValue(defaultUi)
  desktop.saveUiState.mockImplementation(async (ui) => ui)
  desktop.getDataLocation.mockResolvedValue('C:/Synthetic/history.sqlite')
  desktop.readDesktopUsage.mockImplementation(readDemoUsage)
  desktop.readDesktopSessions.mockResolvedValue({ sessions: [], historyError: false })
  desktop.refreshDesktop.mockResolvedValue(undefined)
  desktop.refreshClaude.mockResolvedValue(undefined)
  desktop.refreshAntigravity.mockResolvedValue(undefined)
  desktop.openReleaseUrl.mockResolvedValue(undefined)
})
afterEach(() => {
  vi.unstubAllGlobals()
  localStorage.clear()
  document.documentElement.lang = 'en-US'
  document.documentElement.removeAttribute('data-theme')
  document.documentElement.style.removeProperty('--font-scale')
})

function renderDesktop(quick = false, route = '/settings', localized = false) {
  const client = new QueryClient()
  const app = quick ? <QuickMonitor /> : <MemoryRouter initialEntries={[route]}><App /></MemoryRouter>
  const wrapped = <DisplayProvider>{app}</DisplayProvider>
  render(<QueryClientProvider client={client}>
    {localized ? <LocaleProvider>{wrapped}</LocaleProvider> : wrapped}
  </QueryClientProvider>)
  return userEvent.setup()
}

it('scans on request and connects zero-config Codex in one click from Overview', async () => {
  desktop.scanProviders.mockResolvedValue({ agents: [
    { id: 'claude', executable: 'C:/Synthetic/claude.exe', sourcePath: null, credentialsFound: true, logsFound: true },
    { id: 'codex', executable: 'C:/Synthetic/codex.exe', sourcePath: 'C:/Synthetic/codex.exe', credentialsFound: true, logsFound: true },
  ] })
  desktop.connectProvider.mockResolvedValue({ ...saved, settings: { ...saved.settings, codexEnabled: true } })
  const user = renderDesktop(false, '/')
  expect(desktop.scanProviders).not.toHaveBeenCalled()
  await user.click(screen.getByRole('button', { name: 'Add AI tool' }))
  await user.click(await screen.findByRole('radio', { name: /OpenAI Codex/ }))
  expect(screen.getByText(/zero configuration/)).toBeVisible()
  expect(screen.getByText('C:/Synthetic/codex.exe')).toBeVisible()
  expect(desktop.connectProvider).not.toHaveBeenCalled()
  await user.click(screen.getByRole('button', { name: 'Connect provider' }))
  expect(await screen.findByText(/Tool saved/)).toBeVisible()
  expect(desktop.connectProvider.mock.calls.map(([request]) => request)).toEqual([{ kind: 'codexAuto', executable: 'C:/Synthetic/codex.exe' }])
  expect(desktop.savePreferences).not.toHaveBeenCalled()
})

it('offers a local file for another agent and keeps the interface in Brazilian Portuguese', async () => {
  desktop.scanProviders.mockResolvedValue({ agents: [
    { id: 'opencode', executable: 'C:/Synthetic/opencode.exe', sourcePath: null },
  ] })
  desktop.connectProvider.mockResolvedValue(saved)
  const user = renderDesktop(false, '/settings', true)
  await user.selectOptions(await screen.findByRole('combobox', { name: 'Interface language' }), 'pt-BR')
  expect(document.documentElement.lang).toBe('pt-BR')
  expect(localStorage.getItem('tempoai-language')).toBe('pt-BR')
  expect(desktop.saveUiState).toHaveBeenCalledWith({ language: 'pt-BR' })
  await user.click(screen.getByRole('link', { name: 'Visão geral' }))
  await user.click(screen.getByRole('button', { name: 'Adicionar ferramenta' }))
  await user.click(await screen.findByRole('radio', { name: /OpenCode/ }))
  expect(screen.getByRole('heading', { name: 'Conectar arquivo local de uso' })).toBeVisible()
  const review = screen.getByRole('button', { name: 'Conectar provedor' })
  expect(review).toBeDisabled()
  await user.type(screen.getByRole('textbox', { name: 'Caminho do arquivo JSON de uso' }), 'C:/Synthetic/opencode.json')
  await user.click(review)
  expect(await screen.findByText(/Ferramenta salva/)).toBeVisible()
  expect(desktop.connectProvider.mock.calls.map(([request]) => request)).toEqual([{
    kind: 'local', source: { id: 'local-opencode', name: 'OpenCode', path: 'C:/Synthetic/opencode.json' },
  }])
})

it('keeps a preference draft through route changes without legacy Advanced options or Reduce motion', async () => {
  desktop.savePreferences.mockImplementation(async (settings) => ({ ...saved, settings }))
  const user = renderDesktop()
  expect(await screen.findByRole('heading', { name: 'Settings' })).toBeVisible()
  expect(screen.queryByText('Advanced options')).not.toBeInTheDocument()
  expect(screen.queryByRole('switch', { name: 'Reduce motion' })).not.toBeInTheDocument()

  const shortcut = screen.getByRole('textbox', { name: 'Quick Monitor shortcut' })
  await user.clear(shortcut)
  await user.type(shortcut, 'Ctrl+Alt+K')
  await user.click(screen.getByRole('link', { name: 'Overview' }))
  act(() => events.get('open-settings')?.())
  expect(await screen.findByRole('textbox', { name: 'Quick Monitor shortcut' })).toHaveValue('Ctrl+Alt+K')
  expect(screen.getByText(/Unsaved changes/)).toBeVisible()
  expect(desktop.savePreferences).not.toHaveBeenCalled()

  await user.click(screen.getByRole('button', { name: 'Save changes' }))
  expect(await screen.findByText('Preferences saved.')).toBeVisible()
  expect(desktop.savePreferences).toHaveBeenCalledTimes(1)
  expect(desktop.savePreferences.mock.calls[0][0]).toMatchObject({ shortcut: 'Ctrl+Alt+K' })
})

it('describes load and recovery errors and focuses a field with a rejected save', async () => {
  desktop.getPreferences.mockRejectedValueOnce('unavailable').mockResolvedValueOnce({ ...saved, recoveryWarning: 'Original saved as preferences.json.bak.' })
  const message = 'Enter a valid shortcut, such as Ctrl+Alt+W.'
  desktop.savePreferences.mockRejectedValueOnce({ field: 'shortcut', message })
  const user = renderDesktop()
  expect(await screen.findByRole('alert')).toHaveTextContent('Couldn’t load preferences')
  await user.click(screen.getByRole('button', { name: 'Try again' }))
  expect(await screen.findByText(/preferences.json.bak/)).toBeVisible()
  const shortcut = screen.getByRole('textbox', { name: 'Quick Monitor shortcut' })
  await user.clear(shortcut)
  await user.type(shortcut, 'BadKey')
  await user.click(screen.getByRole('button', { name: 'Save changes' }))
  expect(await screen.findByText(message)).toBeVisible()
  await waitFor(() => expect(shortcut).toHaveFocus())
  expect(shortcut).toHaveAttribute('aria-invalid', 'true')
  expect(shortcut).toHaveAccessibleDescription(message)
})

it('tests saved integrations without submitting the draft and reports each provider outcome', async () => {
  let finish!: (checks: unknown) => void
  desktop.testSavedIntegrations.mockReturnValue(new Promise((resolve) => { finish = resolve }))
  const user = renderDesktop()
  const shortcut = await screen.findByRole('textbox', { name: 'Quick Monitor shortcut' })
  await user.clear(shortcut)
  await user.type(shortcut, 'Ctrl+Alt+Z')
  const button = screen.getByRole('button', { name: 'Check connections' })
  await user.click(button)
  expect(screen.getByText('Checking connections…')).toBeVisible()
  expect(button).toBeDisabled()
  expect(desktop.testSavedIntegrations).toHaveBeenCalledTimes(1)
  expect(desktop.savePreferences).not.toHaveBeenCalled()
  finish([{ providerId: 'claude', status: 'disabled', message: 'Disabled in saved preferences.' },
    { providerId: 'codex', status: 'connected', message: 'Connected.' }])
  expect(await screen.findByText(/Disabled in saved preferences/)).toHaveTextContent('Claude Code')
  expect(screen.getByText(/Connected\./)).toHaveTextContent('OpenAI Codex')
  expect(screen.getByRole('textbox', { name: 'Quick Monitor shortcut' })).toHaveValue('Ctrl+Alt+Z')
})

it('sends only a partial UI patch from Quick Monitor and never reads session history in Quick Monitor', async () => {
  const data = await readDemoUsage()
  Object.assign(data.providers[0].quotas[0], { usedPercent: 70.1, remainingPercent: 29.9 })
  desktop.readDesktopUsage.mockResolvedValue(data)
  desktop.getUiState.mockResolvedValue({ ...defaultUi, perToolPercent: { claude: 'used' } })
  const user = renderDesktop(true)
  expect(await screen.findByText('70.1% used')).toBeVisible()
  expect(desktop.readDesktopSessions).not.toHaveBeenCalled()
  await user.click(screen.getByRole('button', { name: '70.1% used. Show how much is left' }))
  expect(await screen.findByText('29.9% left')).toBeVisible()
  expect(desktop.saveUiState).toHaveBeenCalledWith({ perToolOverride: ['claude', null] })
  await user.keyboard('{Escape}')
  expect(desktop.hidePanel).toHaveBeenCalledTimes(1)
})

it('loads SQLite session history on demand only when navigating to Activity', async () => {
  desktop.readDesktopUsage.mockResolvedValue({ mode: 'live', providers: [], sessions: [], observedAt: null })
  desktop.readDesktopSessions.mockResolvedValue({ sessions: [], historyError: false })
  const user = renderDesktop(false, '/')
  expect(await screen.findByRole('heading', { name: 'Connect your first AI tool' })).toBeVisible()
  expect(desktop.readDesktopSessions).not.toHaveBeenCalled()
  await user.click(screen.getByRole('link', { name: 'Activity' }))
  await waitFor(() => expect(desktop.readDesktopSessions).toHaveBeenCalledTimes(1))
})

it('shows provider skeleton while a connected tool awaits its first live reading', async () => {
  desktop.readDesktopUsage.mockResolvedValue({
    mode: 'live',
    observedAt: null,
    sessions: [],
    providers: [{
      providerId: 'claude',
      displayName: 'Claude Code',
      connectionSaved: true,
      planName: null,
      integration: 'not_connected',
      integrationError: null,
      quotas: [],
      updatedAt: null,
    }],
  })
  renderDesktop(false, '/')
  const card = within(await screen.findByRole('article', { name: 'Claude Code' }))
  expect(card.getByText('Loading…')).toBeVisible()
  expect(card.getByRole('status', { name: 'Loading usage' })).toBeVisible()
})

it('shows how to get current usage and separates old readings from the recommendation', async () => {
  const data = await readDemoUsage()
  const reset = new Date(Date.now() + 3 * 86_400_000)
  const [provider, codex] = data.providers
  provider.integration = 'stale'
  provider.connectionSaved = true
  codex.integration = 'connected'
  Object.assign(provider.quotas[0], {
    status: 'stale', source: 'cli', resetAt: reset.toISOString(), usedPercent: 20,
  })
  Object.assign(codex.quotas[1], { usedPercent: 25, remainingPercent: 75 })
  desktop.readDesktopUsage.mockResolvedValue({ ...data, mode: 'live', providers: [...data.providers, {
    ...provider, providerId: 'antigravity', displayName: 'Google Antigravity',
    quotas: [{ ...provider.quotas[0], type: 'custom', label: '3p-weekly', source: 'cli' }],
  }] })
  const user = renderDesktop(false, '/')
  const card = within(await screen.findByRole('article', { name: 'Claude Code' }))
  expect(card.getByText('Connection saved')).toBeVisible()
  expect(card.getByText('20% used')).toBeVisible()
  await user.click(card.getByRole('button', { name: 'Check now' }))
  expect(await card.findByText(/Claude usage updated\./)).toBeVisible()
  expect(desktop.refreshClaude).toHaveBeenCalledTimes(1)
  expect(desktop.refreshDesktop).not.toHaveBeenCalled()
  const antigravity = within(screen.getByRole('article', { name: 'Google Antigravity' }))
  expect(antigravity.getByText('Weekly limit · Claude and GPT models')).toBeVisible()
  await user.click(antigravity.getByRole('button', { name: 'Check now' }))
  expect(await antigravity.findByText(/Antigravity usage updated\./)).toBeVisible()
  expect(desktop.refreshAntigravity).toHaveBeenCalledTimes(1)
  expect(screen.getByRole('heading', { name: 'OpenAI Codex has usage available' })).toBeVisible()
})

it('removes a tool from the Overview after confirming and keeps the other tools', async () => {
  const data = await readDemoUsage()
  const [claude, codex] = data.providers
  const live = { ...data, mode: 'live' as const, providers: [{ ...claude, connectionSaved: true, integration: 'connected' as const }, { ...codex, connectionSaved: true, integration: 'connected' as const }] }
  desktop.readDesktopUsage.mockResolvedValue(live)
  desktop.removeProvider.mockImplementation(async () => {
    desktop.readDesktopUsage.mockResolvedValue({ ...live, providers: [live.providers[1]] })
    return saved
  })
  const user = renderDesktop(false, '/')
  const card = within(await screen.findByRole('article', { name: 'Claude Code' }))
  await user.click(card.getByRole('button', { name: 'Options for Claude Code' }))
  await user.click(await screen.findByRole('menuitem', { name: 'Remove from TempoAI' }))
  const dialog = within(await screen.findByRole('dialog', { name: 'Remove Claude Code?' }))
  expect(desktop.removeProvider).not.toHaveBeenCalled()
  await user.click(dialog.getByRole('button', { name: 'Remove' }))
  expect(desktop.removeProvider).toHaveBeenCalledWith('claude')
  await waitFor(() => expect(screen.queryByRole('article', { name: 'Claude Code' })).not.toBeInTheDocument())
  expect(screen.getByRole('article', { name: 'OpenAI Codex' })).toBeVisible()
})

it('dismisses the Quick Monitor notice until the situation changes', async () => {
  const data = await readDemoUsage()
  desktop.readDesktopUsage.mockResolvedValue(data)
  const user = renderDesktop(true)
  const notice = await screen.findByRole('region', { name: 'What you can do now' })
  await user.click(within(notice).getByRole('button', { name: 'Dismiss this notice' }))
  expect(screen.queryByRole('region', { name: 'What you can do now' })).not.toBeInTheDocument()
  expect(screen.getByRole('region', { name: 'Claude Code' })).toBeVisible()
  const worse = structuredClone(data)
  Object.assign(worse.providers[1].quotas[1], { usedPercent: 100, remainingPercent: 0 })
  desktop.readDesktopUsage.mockResolvedValue(worse)
  await user.click(screen.getByRole('button', { name: 'Refresh' }))
  expect(await screen.findByRole('heading', { name: 'Limit reached' })).toBeVisible()
})

it('runs first-access onboarding radar scan and connects detected zero-config tools in one click', async () => {
  desktop.readDesktopUsage.mockResolvedValue({ mode: 'live', providers: [], sessions: [], observedAt: null })
  desktop.scanProviders.mockResolvedValue({
    agents: [
      { id: 'claude', executable: 'C:/Synthetic/claude.exe', sourcePath: null, credentialsFound: true, logsFound: true },
      { id: 'codex', executable: null, sourcePath: null, credentialsFound: true, logsFound: true },
      { id: 'antigravity', executable: 'C:/Synthetic/agy.exe', sourcePath: 'C:/Synthetic/agy.exe' },
    ],
  })
  desktop.connectProvider.mockResolvedValue(saved)

  const user = renderDesktop(false, '/')
  expect(await screen.findByRole('heading', { name: 'Connect your first AI tool' })).toBeVisible()
  await user.click(screen.getByRole('button', { name: 'Scan my AI tools' }))
  expect(await screen.findByText(/Found 3 AI tools on this computer/, {}, { timeout: 3_000 })).toBeVisible()
  expect(screen.getByText('C:/Synthetic/agy.exe')).toBeVisible()

  await user.click(screen.getByRole('button', { name: 'Add all found (3)' }))
  await waitFor(() => expect(desktop.connectProvider).toHaveBeenCalledTimes(3))
  expect(desktop.connectProvider.mock.calls.map(([req]) => req)).toEqual([
    { kind: 'claudeAuto' },
    { kind: 'codexAuto', executable: null },
    { kind: 'antigravityAuto', executable: 'C:/Synthetic/agy.exe' },
  ])
})

it('recommends healthy Antigravity buckets and formats its friendly label in Next renewal', async () => {
  const data = await readDemoUsage()
  const [claude] = data.providers
  const soon = new Date(Date.now() + 2 * 3_600_000).toISOString()
  Object.assign(claude.quotas[0], { usedPercent: 100, remainingPercent: 0, resetAt: new Date(Date.now() + 5 * 3_600_000).toISOString() })
  desktop.readDesktopUsage.mockResolvedValue({
    ...data,
    mode: 'live',
    providers: [
      { ...claude, connectionSaved: true, integration: 'connected' },
      {
        providerId: 'antigravity',
        displayName: 'Google Antigravity',
        planName: null,
        connectionSaved: true,
        integration: 'connected',
        updatedAt: data.observedAt,
        quotas: [
          { id: 'agy-1', type: 'custom', label: 'gemini-5h', scope: null, usedPercent: 18, remainingPercent: 82, resetAt: soon, status: 'available', source: 'cli', updatedAt: data.observedAt },
        ],
      },
    ],
  })
  renderDesktop(false, '/')
  const guidance = within(await screen.findByRole('region', { name: 'What you can do now' }))
  expect(guidance.getByText('For your next task, try Google Antigravity.')).toBeVisible()
  const next = within(screen.getByRole('region', { name: 'Next renewal' }))
  expect(next.getByText(/5-hour window · Gemini models/)).toBeVisible()
})

it('scales font size up, down and back to default in Settings and sends partial UI patches', async () => {
  desktop.readDesktopUsage.mockResolvedValue(await readDemoUsage())
  const user = renderDesktop(false, '/settings')
  const increase = await screen.findByRole('button', { name: 'Increase font size' })
  const decrease = screen.getByRole('button', { name: 'Decrease font size' })
  const reset = screen.getByRole('button', { name: 'Reset font size to 100%' })

  expect(reset).toHaveTextContent('100%')
  expect(reset).toBeDisabled()
  await user.click(increase)
  expect(reset).toHaveTextContent('105%')
  expect(reset).toBeEnabled()
  expect(document.documentElement.style.getPropertyValue('--font-scale')).toBe('1.05')
  expect(desktop.saveUiState).toHaveBeenCalledWith({ fontScale: 105 })

  await user.click(decrease)
  await user.click(decrease)
  expect(reset).toHaveTextContent('95%')
  expect(document.documentElement.style.getPropertyValue('--font-scale')).toBe('0.95')
  expect(desktop.saveUiState).toHaveBeenCalledWith({ fontScale: 95 })

  await user.click(reset)
  expect(reset).toHaveTextContent('100%')
  expect(document.documentElement.style.getPropertyValue('--font-scale')).toBe('1')
  expect(desktop.saveUiState).toHaveBeenCalledWith({ fontScale: 100 })
})

it('shows the real Claude connection issue, visible last reading, wait state and explicit check feedback in Quick Monitor', async () => {
  const data = await readDemoUsage()
  const [claude, codex] = data.providers
  const expired = 'Your Claude Code credentials expired. Start a Claude Code session (or open the CLI) and TempoAI will update automatically.'
  const disconnected = {
    ...data,
    mode: 'live' as const,
    providers: [
      {
        ...claude,
        connectionSaved: true,
        integration: 'error' as const,
        integrationError: expired,
        checking: false,
        waitingFor: 'credentials' as const,
        nextRetryAt: null,
        updatedAt: '2026-11-21T14:40:00Z',
        quotas: claude.quotas.map((quota, index) => ({
          ...quota,
          usedPercent: index === 0 ? 18 : 42,
          remainingPercent: index === 0 ? 82 : 58,
          status: 'stale' as const,
        })),
      },
      { ...codex, connectionSaved: true, integration: 'connected' as const },
    ],
  }
  const reconnected = {
    ...data,
    mode: 'live' as const,
    providers: [
      { ...claude, connectionSaved: true, integration: 'connected' as const, integrationError: null, checking: false, waitingFor: null },
      { ...codex, connectionSaved: true, integration: 'connected' as const },
    ],
  }
  desktop.readDesktopUsage.mockResolvedValue(disconnected)
  let resolveRetry!: () => void
  desktop.refreshClaude
    .mockRejectedValueOnce(expired)
    .mockImplementationOnce(() => new Promise<void>((resolve) => {
      resolveRetry = () => {
        desktop.readDesktopUsage.mockResolvedValue(reconnected)
        resolve()
      }
    }))

  const user = renderDesktop(true)
  const card = within(await screen.findByRole('region', { name: 'Claude Code' }))
  expect(card.getByText('Connection issue')).toBeVisible()
  expect(card.getByText(expired)).toBeVisible()
  expect(card.getByText('Waiting for Claude Code to renew · checking automatically every 2s')).toBeVisible()
  expect(card.getByText('18% used')).toBeVisible()
  expect(card.getByText('42% used')).toBeVisible()

  await user.click(card.getByRole('button', { name: 'Check now' }))
  expect(await card.findByRole('alert')).toHaveTextContent(/Claude Code credentials are still expired/)
  expect(desktop.refreshClaude).toHaveBeenCalledTimes(1)

  await user.click(card.getByRole('button', { name: 'Check now' }))
  expect(card.getByRole('button', { name: 'Checking…' })).toBeDisabled()
  resolveRetry()
  expect(await card.findByText('Connected')).toBeVisible()
  expect(card.getByRole('progressbar', { name: 'Claude Code 5-hour window' })).toBeVisible()
  expect(desktop.refreshClaude).toHaveBeenCalledTimes(2)
})

it('shows live retry countdown and active checking status so waits are never blind', async () => {
  const data = await readDemoUsage()
  const [claude, codex] = data.providers
  const retryAt = new Date(Date.now() + 95_000).toISOString()
  desktop.readDesktopUsage.mockResolvedValue({
    ...data,
    mode: 'live',
    providers: [
      {
        ...claude,
        connectionSaved: true,
        integration: 'error',
        integrationError: 'Claude is limiting usage checks right now. TempoAI will try again shortly.',
        checking: false,
        waitingFor: 'retry',
        nextRetryAt: retryAt,
        quotas: claude.quotas.map((quota) => ({ ...quota, status: 'stale' })),
      },
      {
        ...codex,
        connectionSaved: true,
        integration: 'stale',
        checking: true,
        waitingFor: null,
        quotas: codex.quotas.map((quota) => ({ ...quota, status: 'stale' })),
      },
    ],
  })

  renderDesktop(true)
  const claudeCard = within(await screen.findByRole('region', { name: 'Claude Code' }))
  expect(claudeCard.getByText(/Next automatic retry in 1m 3[0-5]s/)).toBeVisible()

  const codexCard = within(screen.getByRole('region', { name: 'OpenAI Codex' }))
  expect(codexCard.getByText('Checking current usage…')).toBeVisible()
  expect(codexCard.getByRole('button', { name: 'Checking…' })).toBeDisabled()
})

it('checks GitHub Releases on demand in Settings and opens the matching .exe download without submitting the draft', async () => {
  const installerUrl = 'https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/download/v0.2.0/TempoAI-0.2.0-setup.exe'
  const portableUrl = 'https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/download/v0.2.0/TempoAI-0.2.0-portable.exe'
  const releaseUrl = 'https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/tag/v0.2.0'
  desktop.checkForUpdates
    .mockResolvedValueOnce({
      status: 'update_available',
      currentVersion: '0.1.0',
      latestVersion: '0.2.0',
      publishedAt: '2026-11-22T12:00:00Z',
      notes: 'Faster Claude reconnect and transparent waits',
      releaseUrl,
      installerUrl,
      portableUrl,
      runningPortable: true,
    })
    .mockResolvedValueOnce({
      status: 'up_to_date',
      currentVersion: '0.2.0',
      latestVersion: '0.2.0',
      publishedAt: '2026-11-22T12:00:00Z',
      notes: null,
      releaseUrl,
      installerUrl,
      portableUrl,
      runningPortable: true,
    })

  const user = renderDesktop(false, '/settings')
  const updates = within(await screen.findByRole('region', { name: 'Updates' }))
  expect(updates.getByText('v0.1.0')).toBeVisible()
  expect(desktop.checkForUpdates).not.toHaveBeenCalled()

  await user.click(updates.getByRole('button', { name: 'Check for updates' }))
  expect(await updates.findByText('Version v0.2.0 available')).toBeVisible()
  expect(updates.getByText('Faster Claude reconnect and transparent waits')).toBeVisible()
  await user.click(updates.getByRole('button', { name: 'Download portable (.exe)' }))
  expect(desktop.openReleaseUrl).toHaveBeenCalledWith(portableUrl)
  await user.click(updates.getByRole('button', { name: 'Download installer (.exe)' }))
  expect(desktop.openReleaseUrl).toHaveBeenCalledWith(installerUrl)
  expect(desktop.savePreferences).not.toHaveBeenCalled()

  await user.click(updates.getByRole('button', { name: 'Check for updates' }))
  expect(await updates.findByText(/TempoAI v0\.2\.0 is up to date\./)).toBeVisible()
})
