import { expect, test } from '@playwright/test'

test('demo workspace adapts to a compact window and supports keyboard navigation and session details', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))
  await page.goto('http://127.0.0.1:43120')
  await expect(page.getByText('Demo data', { exact: true })).toBeVisible()
  const guidance = page.getByRole('region', { name: 'What you can do now' })
  await expect(guidance).toContainText('OpenAI Codex has 14% left for Weekly limit.')
  await expect(guidance).toContainText('For your next task, try Claude Code.')
  await expect(page.getByRole('progressbar')).toHaveCount(4)
  await expect(page.getByRole('region', { name: 'Next renewal' })).toBeVisible()
  await page.setViewportSize({ width: 720, height: 520 })
  expect(await page.locator('main').evaluate((main) => main.scrollWidth)).toBe(await page.locator('main').evaluate((main) => main.clientWidth))
  await page.getByRole('link', { name: 'Activity' }).focus()
  await page.keyboard.press('Enter')
  await expect(page.getByRole('heading', { name: 'Activity' })).toBeFocused()
  await expect(page.locator('.recharts-surface').first()).toBeVisible()
  expect(await page.locator('main').evaluate((main) => main.scrollWidth)).toBe(await page.locator('main').evaluate((main) => main.clientWidth))
  expect((await page.locator('.recharts-surface').first().boundingBox())?.width).toBeGreaterThan(300)
  await page.getByRole('button', { name: '30 days' }).click()
  await expect(page.getByRole('button', { name: '30 days' })).toHaveAttribute('aria-pressed', 'true')
  await page.getByRole('button', { name: '90 days' }).click()
  await expect(page.getByRole('button', { name: '90 days' })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByRole('heading', { name: 'Daily heatmap' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'By project' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'By model' })).toBeVisible()
  await page.getByRole('button', { name: 'Share summary' }).click()
  await expect(page.getByRole('dialog', { name: 'Share Studio' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog', { name: 'Share Studio' })).toHaveCount(0)
  await page.getByRole('button', { name: /View session details/ }).first().click()
  await expect(page.getByRole('dialog', { name: 'Session details' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await page.getByRole('link', { name: 'Settings' }).click()
  expect(await page.locator('main').evaluate((main) => main.scrollWidth)).toBe(await page.locator('main').evaluate((main) => main.clientWidth))
  await expect(page.getByRole('switch', { name: 'Reduce motion' })).toHaveCount(0)
  // Increase font size up to 115% and verify zero horizontal overflow in the compact 720x520 window
  const increaseFont = page.getByRole('button', { name: 'Increase font size' })
  await increaseFont.click()
  await increaseFont.click()
  await increaseFont.click()
  await expect(page.getByRole('button', { name: 'Reset font size to 100%' })).toHaveText('115%')
  expect(await page.locator('main').evaluate((main) => main.scrollWidth)).toBe(await page.locator('main').evaluate((main) => main.clientWidth))
  await page.getByRole('button', { name: 'Reset font size to 100%' }).click()
  await expect(page.getByRole('button', { name: 'Reset font size to 100%' })).toHaveText('100%')
  expect(errors).toEqual([])
})

test('desktop settings keeps the draft and reveals shortcut errors in a compact window', async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 520 })
  await page.addInitScript(() => {
    const settings = {
      autostart: false, minimizeToTray: true, openPanelAtStartup: false, pinPanel: false,
      shortcut: 'Ctrl+Alt+W', claudeEnabled: true, codexEnabled: false, antigravityEnabled: false,
      notifications: false, claudeAlerts: true, codexAlerts: true, antigravityAlerts: true, thresholds: [80, 90, 95, 100],
    }
    const ui = {
      language: null, theme: 'system', fontScale: 100, percentStyle: 'remaining', perToolPercent: {},
      collapsedOverview: [], collapsedQuick: [], lastRoute: '/', activityPeriod: 7, heatmapDays: 90, dismissedGuidance: '',
    }
    Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: {
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
      transformCallback: () => 1,
      invoke: async (command: string) => {
        switch (command) {
          case 'get_preferences': return { settings, shortcutConflict: false, recoveryWarning: null }
          case 'get_ui_state': return ui
          case 'save_ui_state': return ui
          case 'data_location': return 'C:/Synthetic/history.sqlite'
          case 'usage_snapshot': return { mode: 'live', providers: [{ providerId: 'claude', displayName: 'Claude Code', connectionSaved: true, integration: 'connected', planName: null, updatedAt: null,
            quotas: [{ id: 'sample-quota', type: 'five_hour', label: '5-hour window', scope: null, usedPercent: 70.1, remainingPercent: 100 - 70.1, status: 'available', resetAt: null, source: 'cli', updatedAt: null }] }], sessions: [], observedAt: null }
          case 'session_history': return { sessions: [], historyError: false }
          case 'save_preferences': throw { field: 'shortcut', message: 'Enter a valid shortcut, such as Ctrl+Alt+W.' }
          default: return undefined
        }
      },
    } })
  })
  await page.goto('http://127.0.0.1:43120')
  const remaining = page.getByText('29.9% left', { exact: true })
  await expect(remaining).toBeVisible()
  const fitsQuota = await remaining.evaluate((label) => {
    const card = label.closest('.quota')?.getBoundingClientRect()
    return card && label.getBoundingClientRect().right <= card.right
  })
  expect(fitsQuota).toBe(true)
  await page.getByRole('link', { name: 'Settings' }).click()
  await expect(page.getByText('Advanced options')).toHaveCount(0)
  const shortcut = page.getByRole('textbox', { name: 'Quick Monitor shortcut' })
  await shortcut.fill('BadKey')
  await page.getByRole('link', { name: 'Activity' }).click()
  await page.getByRole('link', { name: 'Settings' }).click()
  await expect(shortcut).toHaveValue('BadKey')
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(shortcut).toBeFocused()
  await expect(shortcut).toBeInViewport()
  await expect(shortcut).toHaveAccessibleDescription('Enter a valid shortcut, such as Ctrl+Alt+W.')
  expect(await page.locator('main').evaluate((main) => main.scrollWidth)).toBe(await page.locator('main').evaluate((main) => main.clientWidth))
})

test('production preview has unavailable quotas and no demo activity', async ({ page }) => {
  await page.goto('http://127.0.0.1:43121')
  await expect(page.getByRole('article', { name: 'Claude Code' })).toContainText('Connect this tool to see its usage.')
  await expect(page.getByText('Demo data', { exact: true })).toHaveCount(0)
  await expect(page.getByRole('progressbar')).toHaveCount(0)
  await expect(page.getByRole('region', { name: 'Next renewal' })).toHaveCount(0)
  await page.getByRole('link', { name: 'Activity' }).click()
  await expect(page.getByText('No sessions available')).toBeVisible()
  await expect(page.getByText('No token usage to display')).toBeVisible()
})

test('scans installed agents from a compact Portuguese overview', async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 520 })
  await page.addInitScript(() => {
    const ui = {
      language: null, theme: 'system', fontScale: 100, percentStyle: 'remaining', perToolPercent: {},
      collapsedOverview: [], collapsedQuick: [], lastRoute: '/', activityPeriod: 7, heatmapDays: 90, dismissedGuidance: '',
    }
    Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: {
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
      transformCallback: () => 1,
      invoke: async (command: string) => {
        switch (command) {
          case 'get_preferences': return { settings: { thresholds: [], localProviders: [] }, shortcutConflict: false, recoveryWarning: null }
          case 'get_ui_state': return ui
          case 'save_ui_state': return ui
          case 'data_location': return 'C:/Synthetic/history.sqlite'
          case 'usage_snapshot': return { mode: 'live', providers: [], sessions: [], observedAt: null }
          case 'scan_providers': return { agents: [{ id: 'opencode', executable: 'C:/Synthetic/opencode.exe', sourcePath: null }] }
          default: return undefined
        }
      },
    } })
  })
  await page.goto('http://127.0.0.1:43120/#/settings')
  await page.getByRole('combobox', { name: 'Interface language' }).selectOption('pt-BR')
  await page.getByRole('link', { name: 'Visão geral' }).click()
  await page.reload()
  await expect(page.locator('html')).toHaveAttribute('lang', 'pt-BR')
  await expect(page.getByText('Conecte sua primeira ferramenta de IA')).toBeVisible()
  await page.getByRole('button', { name: 'Escanear minhas IAs' }).click()
  await expect(page.getByText('OpenCode')).toBeVisible({ timeout: 5_000 })
  expect(await page.locator('main').evaluate((main) => main.scrollWidth)).toBe(await page.locator('main').evaluate((main) => main.clientWidth))
  const trigger = page.getByRole('button', { name: 'Adicionar ferramenta' })
  await trigger.click()
  const dialog = page.getByRole('dialog', { name: 'Adicionar ferramenta' })
  await dialog.getByRole('radio', { name: /OpenCode/ }).check()
  await expect(dialog.getByRole('heading', { name: 'Conectar arquivo local de uso' })).toBeVisible()
  expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true)
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
  await expect(trigger).toBeFocused()
})
