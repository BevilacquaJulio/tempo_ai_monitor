import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { expect, it, vi } from 'vitest'
import type { UsageReader } from '@/providers/types'
import { readUnavailableUsage } from '@/services/usage'
import { readDemoUsage } from '@/services/demo-usage'
import { DisplayProvider } from '@/lib/display'
import { App } from './app'

function renderApp(reader: UsageReader, development = false) {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: Infinity } } })
  render(<QueryClientProvider client={client}><DisplayProvider><MemoryRouter><App reader={reader} development={development} /></MemoryRouter></DisplayProvider></QueryClientProvider>)
  return Object.assign(userEvent.setup(), { client })
}

it('shows skeleton loading while reading usage and unavailable production data without demo sessions', async () => {
  let resolveUsage!: (value: Awaited<ReturnType<UsageReader>>) => void
  const deferred: UsageReader = () => new Promise((resolve) => { resolveUsage = resolve })
  const user = renderApp(deferred)
  expect(screen.getByRole('status', { name: 'Loading usage' })).toBeVisible()
  resolveUsage(await readUnavailableUsage())

  const claude = await screen.findByRole('article', { name: 'Claude Code' })
  expect(within(claude).getByRole('region', { name: '5-hour window' })).toHaveTextContent('Unavailable')
  expect(within(claude).getByRole('region', { name: 'Weekly limit' })).toHaveTextContent('Unavailable')
  expect(screen.queryByRole('progressbar')).not.toBeInTheDocument()
  expect(screen.queryByRole('region', { name: 'Next renewal' })).not.toBeInTheDocument()
  expect(screen.queryByText('Demo data')).not.toBeInTheDocument()
  await user.click(screen.getByRole('link', { name: 'Activity' }))
  expect(await screen.findByText('No sessions available')).toBeVisible()
  expect(await screen.findByText('No token usage to display')).toBeVisible()
})

it('labels demo quotas and supports manual refresh, activity filters and session details', async () => {
  const reader = vi.fn(readDemoUsage)
  const user = renderApp(reader, true)
  expect(await screen.findByText('Demo data')).toBeVisible()
  const claude = await screen.findByRole('article', { name: 'Claude Code' })
  expect(within(claude).getByRole('progressbar', { name: '5-hour window usage' })).toHaveAttribute('aria-valuenow', '72')
  expect(within(claude).getByRole('progressbar', { name: 'Weekly limit usage' })).toHaveAttribute('aria-valuenow', '43')
  await user.click(screen.getByRole('button', { name: 'Refresh' }))
  expect(reader).toHaveBeenCalledTimes(2)
  await user.click(screen.getByRole('link', { name: 'Activity' }))
  await user.click(screen.getByRole('button', { name: '30 days' }))
  expect(screen.getByRole('button', { name: '30 days' })).toHaveAttribute('aria-pressed', 'true')
  await user.selectOptions(screen.getByLabelText('Provider'), 'codex')
  await user.selectOptions(screen.getByLabelText('Model'), 'Claude Opus')
  expect(screen.getByText('No matching sessions')).toBeVisible()
  await user.click(screen.getByRole('button', { name: 'Clear filters' }))
  await user.click(screen.getAllByRole('button', { name: /View session details/ })[0])
  expect(screen.getByRole('dialog', { name: 'Session details' })).toHaveTextContent('Demo data')
  await user.keyboard('{Escape}')
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
})

it('retries a failed read and keeps Settings clean without Reduce motion', async () => {
  const reader = vi.fn(readUnavailableUsage).mockRejectedValueOnce(new Error('unavailable source'))
  const user = renderApp(reader)
  expect(await screen.findByRole('alert')).toHaveTextContent('Couldn’t load usage')
  await user.click(screen.getByRole('button', { name: 'Try again' }))
  expect(await screen.findByRole('article', { name: 'Claude Code' })).toBeVisible()
  await user.click(screen.getByRole('link', { name: 'Settings' }))
  expect(screen.getByRole('heading', { name: 'Settings' })).toHaveFocus()
  expect(screen.queryByRole('switch', { name: 'Reduce motion' })).not.toBeInTheDocument()
})

it('keeps quota cards and Activity filters through failed background updates and retry', async () => {
  const reader = vi.fn(readDemoUsage)
  const user = renderApp(reader, true)
  const claude = await screen.findByRole('article', { name: 'Claude Code' })
  reader.mockRejectedValueOnce(new Error('usage temporarily unavailable'))
  await user.click(screen.getByRole('button', { name: 'Refresh' }))
  expect(await screen.findByText(/Last refresh failed/)).toBeVisible()
  expect(within(claude).getByRole('progressbar', { name: '5-hour window usage' })).toBeVisible()

  await user.click(screen.getByRole('link', { name: 'Activity' }))
  await user.click(screen.getByRole('button', { name: '30 days' }))
  await user.selectOptions(screen.getByLabelText('Provider'), 'codex')
  reader.mockRejectedValueOnce(new Error('usage temporarily unavailable'))
  await act(() => user.client.refetchQueries({ queryKey: ['usage'] }))
  expect(await screen.findByText(/Last refresh failed/)).toBeVisible()
  expect(screen.getByLabelText('Provider')).toHaveValue('codex')
  expect(screen.getByRole('button', { name: '30 days' })).toHaveAttribute('aria-pressed', 'true')
  await user.click(screen.getByRole('button', { name: 'Try again' }))
  await waitFor(() => expect(screen.queryByText(/Last refresh failed/)).not.toBeInTheDocument())
  expect(screen.getByLabelText('Provider')).toHaveValue('codex')
})

it('shows newly observed sessions without leaving Activity and reports unreadable history separately', async () => {
  const start = Date.now()
  vi.setSystemTime(start)
  try {
    const data = await readDemoUsage()
    const reader = vi.fn().mockResolvedValue({ ...data, sessions: [], mode: 'live', historyError: true })
    const user = renderApp(reader)
    await screen.findByRole('article', { name: 'Claude Code' })
    await user.click(screen.getByRole('link', { name: 'Activity' }))
    expect(await screen.findByText(/Couldn’t read local session history/)).toBeVisible()
    const lastActivityAt = new Date(start + 10_000).toISOString()
    vi.setSystemTime(start + 15_000)
    reader.mockResolvedValue({ ...data, mode: 'live', observedAt: lastActivityAt, sessions: [
      { ...data.sessions[0], projectName: 'New synthetic session', lastActivityAt },
    ] })
    await act(() => user.client.refetchQueries({ queryKey: ['usage'] }))
    expect(await screen.findByRole('button', { name: /View session details: New synthetic session/ })).toBeVisible()
    expect(screen.queryByText(/Couldn’t read local session history/)).not.toBeInTheDocument()
  } finally {
    vi.useRealTimers()
  }
})

it('renders a continuous high-precision progress bar preserving exact fractional percentages', async () => {
  try {
    const data = await readDemoUsage()
    Object.assign(data.providers[0].quotas[0], { usedPercent: 97, remainingPercent: 3 })
    const user = renderApp(async () => data)
    const claude = await screen.findByRole('article', { name: 'Claude Code' })
    expect(within(claude).getByText('3% left')).toBeVisible()
    const bar = within(claude).getByRole('progressbar', { name: '5-hour window usage' })
    expect(bar).toHaveAttribute('aria-valuenow', '97')
    expect(bar.querySelector('.gauge-fill')).toHaveStyle({ width: '3%' })
    await user.click(within(claude).getByRole('button', { name: '3% left. Show how much was used' }))
    expect(bar.querySelector('.gauge-fill')).toHaveStyle({ width: '97%' })
  } finally {
    localStorage.clear()
  }
})

it('explains which limit ran out and suggests a tool only while its limits show room', async () => {
  const data = await readDemoUsage()
  const [claude, codex] = data.providers
  Object.assign(codex.quotas[1], { usedPercent: 100, remainingPercent: 0, scope: 'GPT Codex' })
  const reader = vi.fn().mockResolvedValue(data)
  const user = renderApp(reader, true)
  const guidance = within(await screen.findByRole('region', { name: 'What you can do now' }))

  expect(guidance.getByRole('heading', { name: 'Limit reached' })).toBeVisible()
  expect(guidance.getByText(/OpenAI Codex.*Weekly limit · GPT Codex.*This limit renews/)).toBeVisible()
  expect(guidance.getByText('For your next task, try Claude Code.')).toBeVisible()

  for (const unconfirmed of [
    { status: 'stale' },
    { status: 'unavailable', usedPercent: null },
    { resetAt: new Date(Date.now() - 1_000).toISOString() },
  ]) {
    reader.mockResolvedValue({ ...data, providers: [
      { ...claude, quotas: [claude.quotas[0], { ...claude.quotas[1], ...unconfirmed }] }, codex,
    ] })
    await act(() => user.client.refetchQueries({ queryKey: ['usage'] }))
    expect(await guidance.findByText(/You can wait until .* for this limit to renew/)).toBeVisible()
    expect(guidance.queryByText(/For your next task/)).not.toBeInTheDocument()
  }
})

it('switches between used and remaining percentages and applies the chosen theme', async () => {
  try {
    const user = renderApp(readDemoUsage, true)
    const claude = within(await screen.findByRole('article', { name: 'Claude Code' }))
    expect(claude.getByText('28% left')).toBeVisible()
    await user.click(claude.getByRole('button', { name: '28% left. Show how much was used' }))
    expect(claude.getByText('72% used')).toBeVisible()
    // Only the clicked tool changes.
    expect(within(screen.getByRole('article', { name: 'OpenAI Codex' })).getByText('62% left')).toBeVisible()
    await user.click(claude.getByRole('button', { name: '72% used. Show how much is left' }))
    expect(claude.getByText('28% left')).toBeVisible()
    await user.click(screen.getByRole('link', { name: 'Settings' }))
    await user.click(screen.getByRole('radio', { name: '27% used' }))
    await user.click(screen.getByRole('radio', { name: 'Dark' }))
    expect(document.documentElement).toHaveAttribute('data-theme', 'dark')
    await user.click(screen.getByRole('link', { name: 'Overview' }))
    const updated = within(await screen.findByRole('article', { name: 'Claude Code' }))
    expect(updated.getByText('72% used')).toBeVisible()
    expect(updated.getByRole('progressbar', { name: '5-hour window usage' })).toHaveAttribute('aria-valuenow', '72')
    await user.click(screen.getByRole('link', { name: 'Settings' }))
    await user.click(screen.getByRole('radio', { name: 'Same as Windows' }))
    expect(document.documentElement).not.toHaveAttribute('data-theme')
    await user.click(screen.getByRole('button', { name: 'Increase font size' }))
    expect(document.documentElement.style.getPropertyValue('--font-scale')).toBe('1.05')
    await user.click(screen.getByRole('button', { name: 'Reset font size to 100%' }))
    expect(document.documentElement.style.getPropertyValue('--font-scale')).toBe('1')
  } finally {
    localStorage.clear()
    document.documentElement.removeAttribute('data-theme')
    document.documentElement.style.removeProperty('--font-scale')
  }
})

it('collapses one tool to its tightest limit and expands it again', async () => {
  try {
    const user = renderApp(readDemoUsage, true)
    const claude = within(await screen.findByRole('article', { name: 'Claude Code' }))
    await user.click(claude.getByRole('button', { name: 'Collapse Claude Code' }))
    expect(claude.queryByRole('progressbar')).not.toBeInTheDocument()
    expect(claude.getByText('5-hour window')).toBeVisible()
    expect(claude.getByText('28% left')).toBeVisible()
    expect(within(screen.getByRole('article', { name: 'OpenAI Codex' })).getAllByRole('progressbar')).toHaveLength(2)
    await user.click(claude.getByRole('button', { name: 'Expand Claude Code' }))
    expect(claude.getAllByRole('progressbar')).toHaveLength(2)
  } finally {
    localStorage.clear()
  }
})

it('shows API-equivalent cost, prompt cache efficiency, 90-day heatmap, breakdown and Share Studio in Activity', async () => {
  try {
    const user = renderApp(readDemoUsage, true)
    await user.click(screen.getByRole('link', { name: 'Activity' }))
    expect(await screen.findByText('API-equivalent cost (est.)')).toBeVisible()
    expect(screen.getByText('Prompt cache efficiency')).toBeVisible()
    expect(screen.getByRole('heading', { name: 'Daily heatmap' })).toBeVisible()
    expect(screen.getByRole('heading', { name: 'By project' })).toBeVisible()
    expect(screen.getByRole('heading', { name: 'By model' })).toBeVisible()

    await user.click(screen.getByRole('button', { name: '90 days' }))
    expect(screen.getByRole('button', { name: '90 days' })).toHaveAttribute('aria-pressed', 'true')
    await user.selectOptions(screen.getByLabelText('Branch'), 'feat/onboarding')
    expect(screen.getByLabelText('Branch')).toHaveValue('feat/onboarding')

    await user.click(screen.getByRole('button', { name: 'Share summary' }))
    const dialog = within(await screen.findByRole('dialog', { name: 'Share Studio' }))
    expect(dialog.getByRole('img', { name: 'Usage summary card preview' })).toBeVisible()
    expect(dialog.getByRole('switch', { name: 'Hide project & branch names' })).toBeChecked()
    await user.click(dialog.getByRole('switch', { name: 'Hide project & branch names' }))
    expect(dialog.getByRole('switch', { name: 'Hide project & branch names' })).not.toBeChecked()
  } finally {
    localStorage.clear()
  }
})
