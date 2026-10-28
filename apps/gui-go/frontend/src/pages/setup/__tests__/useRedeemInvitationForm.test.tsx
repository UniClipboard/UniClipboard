import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { useRedeemInvitationForm } from '@/pages/setup/useRedeemInvitationForm'

describe('useRedeemInvitationForm', () => {
  it.each(['012345', 'ucspace1_AbC-012_xyz'])(
    'submits short codes and full invitations',
    async code => {
      const submit = vi.fn().mockResolvedValue({ ok: true, redeem: null })
      const { result } = renderHook(() => useRedeemInvitationForm(submit))
      act(() => {
        result.current.setCode(code)
        result.current.setPass('space-passphrase')
      })
      expect(result.current.canSubmit).toBe(true)
      await act(async () => {
        await result.current.submit()
      })
      expect(submit).toHaveBeenCalledWith({ code, passphrase: 'space-passphrase' })
    }
  )
})
