import type { IssueInvitationErrorKind } from '@/api/daemon/setupV2'

const translationKeys: Record<IssueInvitationErrorKind, string> = {
  network_not_started: 'setup.invitationIssue.errors.networkNotStarted',
  no_publishable_address: 'setup.invitationIssue.errors.noPublishableAddress',
  local_publication_failed: 'setup.invitationIssue.errors.localPublicationFailed',
  directory_transport_failed: 'setup.invitationIssue.errors.directoryTransportFailed',
  directory_rejected: 'setup.invitationIssue.errors.directoryRejected',
  directory_invalid_response: 'setup.invitationIssue.errors.directoryInvalidResponse',
  service_unavailable: 'setup.invitationIssue.errors.serviceUnavailable',
  internal: 'setup.invitationIssue.errors.internal',
}

export function invitationIssueErrorKey(kind: IssueInvitationErrorKind): string {
  return translationKeys[kind]
}

export function isInvitationIssueRetryable(kind: IssueInvitationErrorKind): boolean {
  return kind !== 'directory_rejected' && kind !== 'internal'
}
