import { useCallback, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { listen } from '@tauri-apps/api/event'
import type { UsageReader } from '@/providers/types'
import { desktop, readDesktopSessions, refreshDesktop } from '@/services/desktop'
import { readUsage } from '@/services/usage'

export function useUsage(reader: UsageReader = readUsage) {
  const query = useQuery({
    queryKey: ['usage'], queryFn: reader, retry: false, staleTime: Infinity,
    refetchInterval: desktop ? 15_000 : false,
  })
  const { refetch } = query
  useEffect(() => {
    if (!desktop) return
    const listeners = ['usage-updated', 'panel-opened'].map((name) => listen(name, () => void refetch()))
    for (const listener of listeners) void listener.catch(() => {})
    return () => {
      for (const listener of listeners) void listener.then((stop) => stop()).catch(() => {})
    }
  }, [refetch])
  const refresh = useCallback(async () => {
    if (desktop) await refreshDesktop().catch(() => {})
    return refetch()
  }, [refetch])
  return { query, refresh }
}

/** Loads SQLite session history on demand only while the Activity view is open. */
export function useSessionHistory(enabled: boolean) {
  const active = desktop && enabled && typeof readDesktopSessions === 'function'
  const query = useQuery({
    queryKey: ['sessions'],
    queryFn: readDesktopSessions,
    enabled: active,
    retry: false,
    staleTime: 15_000,
  })
  const { refetch } = query
  useEffect(() => {
    if (!active) return
    const stop = listen('usage-updated', () => void refetch())
    void stop.catch(() => {})
    return () => { void stop.then((unlisten) => unlisten()).catch(() => {}) }
  }, [active, refetch])
  return query
}
