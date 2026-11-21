import { useMemo, useState } from 'react'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/shared/states'
import { useDisplay } from '@/lib/display'
import { formatCompact, formatNumber, formatUsd } from '@/lib/format'
import { useLocale } from '@/lib/locale'
import type { SessionRecord } from '@/providers/types'
import { heatmapData, type HeatmapCell } from './activity-data'

export function ActivityHeatmap({ sessions, now }: { sessions: SessionRecord[]; now: number }) {
  const { t, language } = useLocale()
  const { heatmapDays: days, setHeatmapDays: setDays } = useDisplay()
  const cells = useMemo(() => heatmapData(sessions, days, now, language), [sessions, days, now, language])
  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const active = cells.find((cell) => cell.key === selectedKey) ?? null
  const activeDays = cells.filter((cell) => cell.sessions > 0).length
  const totalTokens = cells.reduce((sum, cell) => sum + cell.tokens, 0)

  // Group cells into Sunday-to-Saturday week columns, padding the leading partial week.
  const columns = useMemo(() => {
    if (!cells.length) return []
    const leadPad = cells[0].weekday
    const padded: (HeatmapCell | null)[] = [...Array.from({ length: leadPad }, () => null), ...cells]
    const weeks: (HeatmapCell | null)[][] = []
    for (let i = 0; i < padded.length; i += 7) {
      const slice = padded.slice(i, i + 7)
      while (slice.length < 7) slice.push(null)
      weeks.push(slice)
    }
    return weeks
  }, [cells])

  const hasActivity = activeDays > 0

  return (
    <section className="activity-heatmap panel" aria-labelledby="heatmap-title">
      <div className="section-heading">
        <div>
          <h2 id="heatmap-title">{t('Daily heatmap')}</h2>
          <p>
            {hasActivity
              ? t('{active} active days · {tokens} tokens in the last {days} days', {
                active: activeDays,
                tokens: formatCompact(totalTokens, language),
                days,
              })
              : t('Daily intensity across your local AI sessions.')}
          </p>
        </div>
        <div className="segments" role="group" aria-label={t('Heatmap range')}>
          {([30, 90] as const).map((range) => (
            <Button
              key={range}
              variant="ghost"
              aria-pressed={days === range}
              aria-label={t('Show {count}-day heatmap', { count: range })}
              onClick={() => { setDays(range); setSelectedKey(null) }}
            >
              {`${range}d`}
            </Button>
          ))}
        </div>
      </div>

      {!hasActivity ? (
        <EmptyState
          title="No activity recorded yet"
          description="Your daily heatmap will fill in automatically as you use your connected AI tools."
        />
      ) : (
        <>
          <div className="heatmap-scroll">
            <div
              className="heatmap-grid"
              style={{ gridTemplateColumns: `repeat(${columns.length}, minmax(0, 1fr))` }}
              role="grid"
              aria-label={t('Daily activity for the last {days} days', { days })}
            >
              {columns.map((week, weekIndex) => (
                <div className="heatmap-week" key={weekIndex} role="row">
                  {week.map((cell, dayIndex) => {
                    if (!cell) return <span className="heatmap-cell is-pad" key={`pad-${weekIndex}-${dayIndex}`} aria-hidden="true" />
                    const detail = cell.sessions === 0
                      ? t('{date}: No recorded activity', { date: cell.label })
                      : t('{date}: {tokens} tokens · {sessions} sessions', {
                        date: cell.label,
                        tokens: formatNumber(cell.tokens, language),
                        sessions: cell.sessions,
                      })
                    return (
                      <button
                        type="button"
                        key={cell.key}
                        className={`heatmap-cell level-${cell.level}${active?.key === cell.key ? ' is-selected' : ''}`}
                        aria-label={detail}
                        title={detail}
                        onMouseEnter={() => setSelectedKey(cell.key)}
                        onFocus={() => setSelectedKey(cell.key)}
                        onClick={() => setSelectedKey(cell.key)}
                      />
                    )
                  })}
                </div>
              ))}
            </div>
          </div>
          <div className="heatmap-footer">
            <p className="heatmap-detail" aria-live="polite">
              {active
                ? active.sessions === 0
                  ? t('{date}: No recorded activity', { date: active.label })
                  : `${active.label} · ${formatNumber(active.tokens, language)} tokens · ${
                    t(active.sessions === 1 ? '1 session' : '{count} sessions', { count: active.sessions })
                  }${active.costUsd !== null ? ` · ${t('~{cost} API eq.', { cost: formatUsd(active.costUsd, language) })}` : ''}`
                : t('Hover or select a day to inspect its totals.')}
            </p>
            <div className="heatmap-legend" aria-hidden="true">
              <span>{t('Less')}</span>
              {[0, 1, 2, 3, 4].map((level) => <span key={level} className={`heatmap-swatch level-${level}`} />)}
              <span>{t('More')}</span>
            </div>
          </div>
        </>
      )}
    </section>
  )
}
