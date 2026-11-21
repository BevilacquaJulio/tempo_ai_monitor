import type { ReactNode } from 'react'
import { useLocale } from '@/lib/locale'

export function PageHeading({ title, description, children }: { title: string; description: string; children?: ReactNode }) {
  const { t } = useLocale()
  return <header className="page-heading">
    <div><h1 tabIndex={-1}>{t(title)}</h1><p>{t(description)}</p></div>
    {children && <div className="page-actions">{children}</div>}
  </header>
}
