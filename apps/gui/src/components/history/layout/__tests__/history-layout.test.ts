import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  clampListWidth,
  DETAIL_COLUMN_MIN,
  historyLayoutTier,
  LIST_COLUMN,
  readListWidth,
  writeListWidth,
} from '../history-layout'

describe('history layout tiers', () => {
  it('maps window widths to tiers at 1100 and 1440', () => {
    expect(historyLayoutTier(900)).toBe('compact')
    expect(historyLayoutTier(1099)).toBe('compact')
    expect(historyLayoutTier(1100)).toBe('standard')
    expect(historyLayoutTier(1439)).toBe('standard')
    expect(historyLayoutTier(1440)).toBe('wide')
    expect(historyLayoutTier(2560)).toBe('wide')
  })

  it('fits the narrowest list and the detail floor into the 900px window minimum', () => {
    // The compact tier hides the sidebar.
    expect(LIST_COLUMN.compact.min + DETAIL_COLUMN_MIN).toBeLessThanOrEqual(900)
    // Standard tier opens at 1100 with the 220px sidebar.
    expect(220 + LIST_COLUMN.standard.min + DETAIL_COLUMN_MIN).toBeLessThanOrEqual(1100)
  })

  it('keeps the design anchor: a 560px list in the standard and wide tiers', () => {
    expect(LIST_COLUMN.standard.default).toBe(560)
    expect(LIST_COLUMN.wide.default).toBe(560)
  })

  it('clamps widths into the tier range', () => {
    expect(clampListWidth('compact', 900)).toBe(520)
    expect(clampListWidth('compact', 100)).toBe(360)
    expect(clampListWidth('wide', 612.4)).toBe(612)
  })
})

describe('remembered list width', () => {
  afterEach(() => {
    localStorage.clear()
    vi.restoreAllMocks()
  })

  it('remembers one width per tier, clamped on the way in and out', () => {
    writeListWidth('wide', 650)
    writeListWidth('compact', 999)

    expect(readListWidth('wide')).toBe(650)
    expect(readListWidth('compact')).toBe(520)
    expect(readListWidth('standard')).toBeNull()
  })

  it('falls back to no memory when storage is unreadable or throws', () => {
    localStorage.setItem('uc.history.listWidth.v1', '{not json')
    expect(readListWidth('wide')).toBeNull()

    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(() => writeListWidth('wide', 600)).not.toThrow()
  })
})
