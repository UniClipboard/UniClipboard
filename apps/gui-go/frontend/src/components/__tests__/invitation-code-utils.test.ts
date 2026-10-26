import { describe, expect, it } from 'vitest'
import {
  formatInvitationCode,
  classifyInvitationInput,
  normalizeInvitationInput,
} from '@/lib/invitation-code'

describe('six-digit invitation display', () => {
  it.each(['000001', '000-001'])('preserves leading zeros in %s', raw => {
    expect(formatInvitationCode(raw)).toBe('000-001')
  })
})

describe('invitation input', () => {
  it.each(['012345', '012-345', ' 012 - 345 '])(
    'accepts a short code without losing zeros: %s',
    input => {
      expect(classifyInvitationInput(input)).toBe('short_code')
      expect(normalizeInvitationInput(input)).toBe('012-345')
    }
  )

  it('keeps a full invitation opaque, trimming only the surrounding whitespace', () => {
    const invitation = 'ucspace1_AbC-012_xyz'
    expect(classifyInvitationInput(` \n${invitation}\t`)).toBe('full_invitation')
    expect(normalizeInvitationInput(` \n${invitation}\t`)).toBe(invitation)
    expect(normalizeInvitationInput(' ucspace1_Ab C-012 ')).toBe('ucspace1_Ab C-012')
  })

  it.each(['', '01234', '0123456', 'abcdef', 'UCSPACE1_AbC', 'ucspace1_'])(
    'rejects incomplete or unsupported input: %s',
    input => {
      expect(classifyInvitationInput(input)).toBe('invalid')
    }
  )
})
