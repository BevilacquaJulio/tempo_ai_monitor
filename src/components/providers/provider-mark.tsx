import { Bot, Sparkle, Terminal } from 'lucide-react'
import type { ProviderId } from '@/providers/types'

export function ProviderMark({ id }: { id: ProviderId }) {
  const Icon = id === 'claude' ? Sparkle : id === 'codex' ? Terminal : Bot
  return <span className={`provider-mark provider-${id}`}><Icon aria-hidden="true" strokeWidth={1.7} /></span>
}
