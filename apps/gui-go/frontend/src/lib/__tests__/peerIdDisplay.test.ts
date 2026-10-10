import { describe, expect, it } from 'vitest'
import { formatPeerIdForDisplay } from '@/lib/utils'

describe('formatPeerIdForDisplay', () => {
  it('returns suffix for long peer IDs', () => {
    expect(formatPeerIdForDisplay('12D3KooWABCDEFGH')).toBe('ABCDEFGH')
  })

  it('returns full value for short peer IDs', () => {
    expect(formatPeerIdForDisplay('peer-1')).toBe('peer-1')
  })

  it('returns empty string for missing peer IDs', () => {
    expect(formatPeerIdForDisplay('')).toBe('')
    expect(formatPeerIdForDisplay(undefined)).toBe('')
    expect(formatPeerIdForDisplay(null)).toBe('')
  })
})
