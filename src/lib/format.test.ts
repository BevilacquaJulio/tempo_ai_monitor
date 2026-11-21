import { expect, it } from 'vitest'
import { formatExpectedRenewal, formatRenewal, formatRenewalUntil } from './format'

it('formats renewal with today, weekday for 1-6 days ahead, and date for 7+ days ahead', () => {
  // Saturday, Nov 21, 2026 10:00 local time
  const now = new Date(2026, 10, 21, 10, 0).getTime()
  const sameDay = new Date(2026, 10, 21, 14, 30).getTime()
  const in2DaysMon = new Date(2026, 10, 23, 14, 30).getTime()
  const in6DaysFri = new Date(2026, 10, 27, 9, 5).getTime()
  const in7DaysSat = new Date(2026, 10, 28, 18, 45).getTime()

  expect(formatRenewal(sameDay, now, 'pt-BR')).toBe('Renova hoje às 14:30')
  expect(formatRenewal(in2DaysMon, now, 'pt-BR')).toBe('Renova em seg às 14:30')
  expect(formatRenewal(in6DaysFri, now, 'pt-BR')).toBe('Renova em sex às 09:05')
  expect(formatRenewal(in7DaysSat, now, 'pt-BR')).toBe('Renova em 28/11 às 18:45')
  expect(formatExpectedRenewal(in7DaysSat, now, 'pt-BR')).toBe('Previsão: renova em 28/11 às 18:45')
  expect(formatRenewalUntil(sameDay, now, 'pt-BR')).toBe('hoje às 14:30')
  expect(formatRenewalUntil(in2DaysMon, now, 'pt-BR')).toBe('seg às 14:30')
  expect(formatRenewalUntil(in7DaysSat, now, 'pt-BR')).toBe('28/11 às 18:45')

  expect(formatRenewal(sameDay, now, 'en-US')).toMatch(/^Renews today at 2:30\s?PM$/i)
  expect(formatRenewal(in2DaysMon, now, 'en-US')).toMatch(/^Renews Mon at 2:30\s?PM$/i)
  expect(formatRenewal(in6DaysFri, now, 'en-US')).toMatch(/^Renews Fri at 9:05\s?AM$/i)
  expect(formatRenewal(in7DaysSat, now, 'en-US')).toMatch(/^Renews on Nov 28 at 6:45\s?PM$/i)
  expect(formatRenewalUntil(in7DaysSat, now, 'en-US')).toMatch(/^Nov 28 at 6:45\s?PM$/i)
})
