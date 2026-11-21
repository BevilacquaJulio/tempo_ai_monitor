import { formatCompact, formatPercent, formatUsd } from '@/lib/format'
import { useLocale } from '@/lib/locale'
import type { BreakdownRow } from './activity-data'

function BreakdownList({ title, subtitle, rows, emptyText }: {
  title: string
  subtitle: string
  rows: BreakdownRow[]
  emptyText: string
}) {
  const { t, language } = useLocale()
  const top = rows.slice(0, 6)
  return (
    <section className="breakdown-card panel" aria-label={t(title)}>
      <div className="breakdown-head">
        <h3>{t(title)}</h3>
        <p>{t(subtitle)}</p>
      </div>
      {top.length === 0 ? (
        <p className="breakdown-empty">{t(emptyText)}</p>
      ) : (
        <ul className="breakdown-list">
          {top.map((row) => (
            <li key={row.name} className="breakdown-row">
              <div className="breakdown-row-top">
                <span className="breakdown-name" title={row.name}>{row.name}</span>
                <span className="breakdown-figures">
                  {row.tokens !== null ? `${formatCompact(row.tokens, language)} tokens` : t(row.sessions === 1 ? '1 session' : '{count} sessions', { count: row.sessions })}
                  {row.costUsd !== null && <span className="breakdown-cost">{formatUsd(row.costUsd, language)}</span>}
                </span>
              </div>
              <div className="breakdown-bar" aria-hidden="true">
                <span style={{ width: `${Math.max(4, Math.min(100, row.share))}%` }} />
              </div>
              <div className="breakdown-meta">
                <span>{t(row.sessions === 1 ? '1 session' : '{count} sessions', { count: row.sessions })}</span>
                {row.cacheHitRate !== null && (
                  <span>{t('{value}% cache hit', { value: formatPercent(row.cacheHitRate, language) })}</span>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

export function UsageBreakdown({ projects, models }: { projects: BreakdownRow[]; models: BreakdownRow[] }) {
  return (
    <div className="breakdown-grid">
      <BreakdownList
        title="By project"
        subtitle="Folders where your AI tools worked most in this period."
        rows={projects}
        emptyText="Project totals will appear once sessions are recorded."
      />
      <BreakdownList
        title="By model"
        subtitle="Token volume, cache efficiency and API-equivalent estimate per model."
        rows={models}
        emptyText="Model breakdown will appear once sessions report model names."
      />
    </div>
  )
}
