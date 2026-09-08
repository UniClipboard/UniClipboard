import { describe, expect, it } from 'vitest'
import { formatInvitationCode } from '@/lib/invitation-code'

describe('six-digit invitation display', () => {
  it.each(['000001', '000-001'])('preserves leading zeros in %s', raw => {
    expect(formatInvitationCode(raw)).toBe('000-001')
  })
})
