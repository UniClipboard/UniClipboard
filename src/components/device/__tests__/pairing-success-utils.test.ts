import { describe, expect, it } from 'vitest'
import { hasNewPairedMember } from '@/components/device/pairing-success-utils'

describe('hasNewPairedMember', () => {
  it('recognizes a newly admitted device after workspace convergence changes', () => {
    expect(hasNewPairedMember(2, 3)).toBe(true)
  })

  it('does not treat ordinary convergence without a new member as pairing success', () => {
    expect(hasNewPairedMember(2, 2)).toBe(false)
  })
})
