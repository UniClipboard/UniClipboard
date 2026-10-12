import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { IssueInvitationErrorKind } from '@/api/daemon/setupV2'
import { invitationIssueErrorKey, isInvitationIssueRetryable } from '@/lib/invitation-issue-error'

export type IssueInvitation = () => Promise<
  { ok: true } | { ok: false; kind: IssueInvitationErrorKind; raw: string }
>

/** Issuing the first invitation from the space-ready step; non-retryable failures disable it. */
export function useSpaceReadyInvite(onInvite: IssueInvitation) {
  const { t } = useTranslation()
  const [inviteError, setInviteError] = useState<string | null>(null)
  const [inviteRetryable, setInviteRetryable] = useState(true)

  const invite = async () => {
    setInviteError(null)
    setInviteRetryable(true)
    const result = await onInvite()
    if (!result.ok) {
      setInviteError(t(invitationIssueErrorKey(result.kind)))
      setInviteRetryable(isInvitationIssueRetryable(result.kind))
    }
  }

  return { inviteError, inviteRetryable, invite }
}
