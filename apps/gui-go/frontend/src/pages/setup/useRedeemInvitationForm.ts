import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import type { ActiveJoinSpaceResponse, RedeemInvitationErrorKind } from '@/api/daemon/setupV2'
import { classifyInvitationInput } from '@/lib/invitation-code'
import { redeemErrorMessage } from './setup-error-messages'

export type RedeemInvitationSubmit = (input: {
  code: string
  passphrase: string
}) => Promise<
  | { ok: true; redeem: ActiveJoinSpaceResponse | null }
  | { ok: false; kind: RedeemInvitationErrorKind; raw: string }
>

/** Form state and submission of the "join an existing space" step, shared by every skin. */
export function useRedeemInvitationForm(onSubmit: RedeemInvitationSubmit, loading?: boolean) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.redeemInvitation' })
  const [code, setCode] = useState('')
  const [pass, setPass] = useState('')
  const [errorKind, setErrorKind] = useState<RedeemInvitationErrorKind | null>(null)

  const codeComplete = classifyInvitationInput(code) !== 'invalid'
  const canSubmit = codeComplete && pass.length > 0 && !loading

  const submit = async (event?: FormEvent) => {
    event?.preventDefault()
    setErrorKind(null)
    if (!canSubmit) return
    const res = await onSubmit({ code, passphrase: pass })
    if (!res.ok) {
      setErrorKind(res.kind)
      // These failures all consume or invalidate the one-time invitation.
      if (
        res.kind === 'invitation_not_found' ||
        res.kind === 'invitation_expired' ||
        res.kind === 'sponsor_rejected' ||
        res.kind === 'passphrase_mismatch'
      ) {
        setCode('')
        setPass('')
      }
    }
  }

  return {
    code,
    setCode,
    pass,
    setPass,
    codeComplete,
    canSubmit,
    codeInvalid: errorKind === 'invitation_not_found' || errorKind === 'invitation_expired',
    errorMessage: redeemErrorMessage(t, errorKind),
    submit,
  }
}
