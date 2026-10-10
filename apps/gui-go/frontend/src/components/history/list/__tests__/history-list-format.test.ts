import { describe, expect, it } from 'vitest'
import { dayKey, formatClockTime, formatDayHeading } from '../history-list-format'

const t = (key: string) => key
// Local-time constructor so the assertions hold in any test-runner time zone.
const at = (y: number, m: number, d: number, h = 12, min = 0, s = 0) =>
  new Date(y, m - 1, d, h, min, s).getTime()

describe('history list format', () => {
  const now = at(2026, 9, 28, 9, 40)

  it('titles today and yesterday by calendar day, not by 24h window', () => {
    expect(formatDayHeading(at(2026, 9, 28, 0, 1), now, 'en-US', t)).toEqual({
      title: 'history.timeRange.today',
      detail: 'Mon, Sep 28',
    })
    // 23:59 the day before is under 24h ago but still yesterday.
    expect(formatDayHeading(at(2026, 9, 27, 23, 59), now, 'en-US', t)).toEqual({
      title: 'history.timeRange.yesterday',
      detail: 'Sun, Sep 27',
    })
  })

  it('titles the past week by weekday, older days by date with the year outside this one', () => {
    expect(formatDayHeading(at(2026, 9, 22), now, 'en-US', t)).toEqual({
      title: 'Tuesday',
      detail: 'Sep 22',
    })
    expect(formatDayHeading(at(2026, 9, 15), now, 'en-US', t)).toEqual({
      title: 'Sep 15',
      detail: 'Tue',
    })
    expect(formatDayHeading(at(2025, 12, 31), now, 'en-US', t)).toEqual({
      title: 'Dec 31, 2025',
      detail: 'Wed',
    })
  })

  it('groups timestamps of the same local day under one key', () => {
    expect(dayKey(at(2026, 9, 28, 0, 0))).toBe(dayKey(at(2026, 9, 28, 23, 59)))
    expect(dayKey(at(2026, 9, 28, 0, 0))).not.toBe(dayKey(at(2026, 9, 27, 23, 59)))
  })

  it('formats the row time as 24-hour clock time with seconds', () => {
    expect(formatClockTime(at(2026, 9, 27, 22, 14, 8), 'en-US')).toBe('22:14:08')
    expect(formatClockTime(at(2026, 9, 28, 0, 5, 9), 'en-US')).toBe('00:05:09')
  })
})
