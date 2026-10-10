type Translate = (key: string, opts?: Record<string, unknown>) => string

const DAY_MS = 24 * 60 * 60 * 1000

function startOfDay(ms: number): number {
  const date = new Date(ms)
  date.setHours(0, 0, 0, 0)
  return date.getTime()
}

/** Local calendar day of a ms timestamp, for grouping rows under one header. */
export function dayKey(ms: number): number {
  return startOfDay(ms)
}

/** HList.dc.html day header: a relative title ("Today", "Yesterday", the
 * weekday within the past week, else the date) and a quieter date detail. */
export function formatDayHeading(
  ms: number,
  now: number,
  locale: string,
  t: Translate
): { title: string; detail: string } {
  const day = startOfDay(ms)
  // Compare calendar days, not 24h windows, so DST shifts do not misfile a row.
  const daysAgo = Math.round((startOfDay(now) - day) / DAY_MS)
  const year = new Date(ms).getFullYear() === new Date(now).getFullYear() ? undefined : 'numeric'
  const format = (options: Intl.DateTimeFormatOptions) =>
    new Intl.DateTimeFormat(locale, options).format(ms)
  const fullDate = format({ weekday: 'short', month: 'short', day: 'numeric', year })
  if (daysAgo === 0) return { title: t('history.timeRange.today'), detail: fullDate }
  if (daysAgo === 1) return { title: t('history.timeRange.yesterday'), detail: fullDate }
  if (daysAgo > 1 && daysAgo < 7) {
    return {
      title: format({ weekday: 'long' }),
      detail: format({ month: 'short', day: 'numeric' }),
    }
  }
  return {
    title: format({ month: 'short', day: 'numeric', year }),
    detail: format({ weekday: 'short' }),
  }
}

/** 24-hour wall-clock time with seconds, as the row meta line shows it. */
export function formatClockTime(ms: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hourCycle: 'h23',
  }).format(ms)
}

/** Month, day and clock time, as the detail's Copied card shows it. */
export function formatCopiedAt(ms: number, locale: string): string {
  const date = new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' }).format(ms)
  return `${date} · ${formatClockTime(ms, locale)}`
}
