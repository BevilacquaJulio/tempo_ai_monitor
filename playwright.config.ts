import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  use: { channel: 'msedge', locale: 'en-US', viewport: { width: 1180, height: 800 } },
  webServer: [
    { command: 'npm run dev -- --port 43120', url: 'http://127.0.0.1:43120', timeout: 30_000 },
    { command: 'npm run preview -- --host 127.0.0.1 --port 43121 --strictPort', url: 'http://127.0.0.1:43121', timeout: 30_000 },
  ],
})
