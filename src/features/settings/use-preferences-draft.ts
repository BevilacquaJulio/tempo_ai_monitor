import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { savePreferences, type PreferenceError, type Preferences } from '@/services/desktop'

export function usePreferencesDraft(refresh: () => Promise<unknown>) {
  const [draft, setDraft] = useState<Partial<Preferences>>({})
  const client = useQueryClient()
  const save = useMutation({
    mutationFn: (settings: Preferences) => savePreferences({
      ...settings, localProviders: (settings.localProviders ?? []).map((source) => ({ ...source, path: source.path.trim() })),
    }),
    onSuccess: async (saved) => {
      await client.cancelQueries({ queryKey: ['preferences'], exact: true })
      client.setQueryData(['preferences'], saved)
      setDraft({})
      await refresh()
    },
  })
  const set = <K extends keyof Preferences>(key: K, value: Preferences[K]) => {
    save.reset()
    setDraft((current) => ({ ...current, [key]: value }))
  }
  const error: PreferenceError | undefined = save.isError
    ? typeof save.error?.message === 'string' ? save.error : { message: 'Could not save preferences.' }
    : undefined
  const discard = () => { save.reset(); setDraft({}) }
  return { draft, set, save, error, discard }
}
