import { useLocale } from '@/lib/locale'
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { EmptyState } from '@/components/shared/states'
import { formatCompact, formatNumber } from '@/lib/format'
import type { dailyUsage } from './activity-data'

export default function TokenChart({ data, account }: { data: ReturnType<typeof dailyUsage>; account?: boolean }) {
  const { t, language } = useLocale()
  if (!data.some((day) => day.tokens !== null)) {
    return <EmptyState title="No token usage to display" description="Token totals will appear when a source provides them for the selected sessions." />
  }

  return <figure>
    <div className="chart-canvas">
      <ResponsiveContainer width="100%" height="100%" minWidth={0} minHeight={0}>
        <BarChart data={data} margin={{ top: 8, right: 6, bottom: 0, left: 0 }} accessibilityLayer>
          <CartesianGrid vertical={false} stroke="var(--border)" strokeDasharray="3 4" />
          <XAxis dataKey="label" tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} axisLine={false} tickLine={false} minTickGap={20} />
          <YAxis width={44} tickCount={4} tickFormatter={(value: number) => formatCompact(value, language)} tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} axisLine={false} tickLine={false} />
          <Tooltip cursor={{ fill: 'var(--card)' }} contentStyle={{ backgroundColor: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, color: 'var(--foreground)' }} itemStyle={{ color: 'var(--foreground)' }} formatter={(value) => formatNumber(typeof value === 'number' ? value : null, language)} />
          <Bar dataKey="tokens" name="Tokens" fill="var(--primary)" radius={[4, 4, 0, 0]} maxBarSize={45} isAnimationActive={false} />
        </BarChart>
      </ResponsiveContainer>
    </div>
    <figcaption className="chart-caption">{t(account ? 'Days are reported by Codex. Account totals are not attributed to individual sessions.' : 'Totals are grouped by the session’s last activity, in local time.')} {t("Blank days have no known total.")}</figcaption>
    <details className="chart-values"><summary>{t("View daily totals")}</summary>
      <dl>{data.map(({ label, tokens }) => <div key={label}><dt>{label}</dt><dd>{formatNumber(tokens, language)}</dd></div>)}</dl>
    </details>
  </figure>
}
