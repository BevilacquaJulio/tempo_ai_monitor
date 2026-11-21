import { unconnectedProviders } from '@/providers/registry'
import type { UsageReader } from '@/providers/types'
import { desktop, readDesktopUsage } from './desktop'

export const readUnavailableUsage: UsageReader = async () => ({
  mode: 'live',
  providers: unconnectedProviders(),
  sessions: [],
  observedAt: null,
})

export const readUsage: UsageReader = desktop ? readDesktopUsage : import.meta.env.DEV
  ? async () => (await import('./demo-usage')).readDemoUsage()
  : readUnavailableUsage
