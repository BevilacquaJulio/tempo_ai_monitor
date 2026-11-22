import type { Language } from './locale'

function formats(language: Language) {
  return {
    number: new Intl.NumberFormat(language),
    compact: new Intl.NumberFormat(language, { notation: 'compact', maximumFractionDigits: 1 }),
    percent: new Intl.NumberFormat(language, { maximumFractionDigits: 1 }),
    usd: new Intl.NumberFormat(language, { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    usdSmall: new Intl.NumberFormat(language, { style: 'currency', currency: 'USD', minimumFractionDigits: 3, maximumFractionDigits: 4 }),
    date: new Intl.DateTimeFormat(language, { month: 'short', day: 'numeric' }),
    weekday: new Intl.DateTimeFormat(language, { weekday: 'short', hour: 'numeric', minute: '2-digit' }),
    time: new Intl.DateTimeFormat(language, { hour: 'numeric', minute: '2-digit' }),
    timeSeconds: new Intl.DateTimeFormat(language, { hour: 'numeric', minute: '2-digit', second: '2-digit' }),
    time24: new Intl.DateTimeFormat('pt-BR', { hour: '2-digit', minute: '2-digit', hour12: false }),
    dateTime: new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeStyle: 'short' }),
  }
}
const localeFormats = { 'en-US': formats('en-US'), 'pt-BR': formats('pt-BR') }
const ptWeekdays = ['dom', 'seg', 'ter', 'qua', 'qui', 'sex', 'sáb'] as const
const enWeekdays = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'] as const

export const formatNumber = (value: number | null, language: Language = 'en-US') => value === null ? (language === 'pt-BR' ? 'Indisponível' : 'Unavailable') : localeFormats[language].number.format(value)
export const formatCompact = (value: number, language: Language = 'en-US') => localeFormats[language].compact.format(value)
export const formatPercent = (value: number, language: Language = 'en-US') => localeFormats[language].percent.format(value)
export const formatUsd = (value: number | null, language: Language = 'en-US') => {
  if (value === null || !Number.isFinite(value)) return language === 'pt-BR' ? 'Indisponível' : 'Unavailable'
  return localeFormats[language][value > 0 && value < 0.01 ? 'usdSmall' : 'usd'].format(value)
}
export const formatDay = (value: number | string, language: Language = 'en-US') => localeFormats[language].date.format(new Date(value))
export const formatTime = (value: number | string, language: Language = 'en-US') => localeFormats[language].time.format(new Date(value))
export const formatTimeWithSeconds = (value: number | string, language: Language = 'en-US') => localeFormats[language].timeSeconds.format(new Date(value))
export const formatDateTime = (value: number | string, language: Language = 'en-US') => localeFormats[language].dateTime.format(new Date(value))
export const formatReset = (value: number, weekly: boolean, language: Language = 'en-US') => localeFormats[language][weekly ? 'weekday' : 'time'].format(value)

function calendarDaysBetween(from: number, to: number) {
  const start = new Date(from)
  const end = new Date(to)
  const startUtc = Date.UTC(start.getFullYear(), start.getMonth(), start.getDate())
  const endUtc = Date.UTC(end.getFullYear(), end.getMonth(), end.getDate())
  return Math.round((endUtc - startUtc) / 86_400_000)
}

/**
 * Formats when a quota renews (never a countdown):
 * - Same day: "hoje às 14:30" / "today at 2:30 PM"
 * - 1..6 calendar days ahead: "em seg às 14:30" / "Mon at 2:30 PM"
 * - 7+ calendar days ahead: "em 28/11 às 14:30" / "on Nov 28 at 2:30 PM"
 */
export function formatRenewalWhen(at: number, now: number, language: Language = 'en-US'): string {
  const target = new Date(at)
  const days = calendarDaysBetween(now, at)
  if (language === 'pt-BR') {
    const clock = localeFormats['pt-BR'].time24.format(target)
    if (days <= 0) return `hoje às ${clock}`
    if (days <= 6) return `em ${ptWeekdays[target.getDay()]} às ${clock}`
    const day = String(target.getDate()).padStart(2, '0')
    const month = String(target.getMonth() + 1).padStart(2, '0')
    return `em ${day}/${month} às ${clock}`
  }
  const clock = localeFormats['en-US'].time.format(target)
  if (days <= 0) return `today at ${clock}`
  if (days <= 6) return `${enWeekdays[target.getDay()]} at ${clock}`
  return `on ${localeFormats['en-US'].date.format(target)} at ${clock}`
}

/** Same horizon rules without leading "em "/"on ", for phrases like "até {time}" / "until {time}". */
export function formatRenewalUntil(at: number, now: number, language: Language = 'en-US'): string {
  return formatRenewalWhen(at, now, language).replace(/^(?:em|on)\s+/, '')
}

export function formatRenewal(at: number, now: number, language: Language = 'en-US'): string {
  const when = formatRenewalWhen(at, now, language)
  return language === 'pt-BR' ? `Renova ${when}` : `Renews ${when}`
}

export function formatExpectedRenewal(at: number, now: number, language: Language = 'en-US'): string {
  const when = formatRenewalWhen(at, now, language)
  return language === 'pt-BR' ? `Previsão: renova ${when}` : `Expected: renews ${when}`
}

export function formatDuration(start: string | null, end: string) {
  if (!start) return 'Unavailable'
  const minutes = Math.floor((Date.parse(end) - Date.parse(start)) / 60_000)
  if (!Number.isFinite(minutes) || minutes < 0) return 'Unavailable'
  if (minutes < 1) return '<1 min'
  return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)}h ${minutes % 60}m`
}
