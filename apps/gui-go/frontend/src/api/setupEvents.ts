/** Typed subscribers for setup invitations and re-pairing notifications. */

import { onDaemonRealtimeEvent } from '@/api/realtime'

export interface SetupInvitationIssuedEvent {
  code: string
  expiresAtMs: number
}

export interface SetupInvitationRevokedEvent {
  /** Backend-supplied reason such as `"cancelled"` or `"expired"`. */
  reason: string
}

export interface SetupRePairingRequiredEvent {
  scope: 'all_devices'
}

const TOPIC = 'setup'
const EVT_INVITATION_ISSUED = 'setup.invitationIssued'
const EVT_INVITATION_REVOKED = 'setup.invitationRevoked'
const EVT_RE_PAIRING_REQUIRED = 'setup.rePairingRequired'

export async function onSetupInvitationIssued(
  callback: (event: SetupInvitationIssuedEvent) => void
): Promise<() => void> {
  return onDaemonRealtimeEvent(event => {
    if (event.topic !== TOPIC || event.type !== EVT_INVITATION_ISSUED) return
    callback(event.payload as SetupInvitationIssuedEvent)
  })
}

export async function onSetupInvitationRevoked(
  callback: (event: SetupInvitationRevokedEvent) => void
): Promise<() => void> {
  return onDaemonRealtimeEvent(event => {
    if (event.topic !== TOPIC || event.type !== EVT_INVITATION_REVOKED) return
    callback(event.payload as SetupInvitationRevokedEvent)
  })
}

export async function onSetupRePairingRequired(
  callback: (event: SetupRePairingRequiredEvent) => void
): Promise<() => void> {
  return onDaemonRealtimeEvent(event => {
    if (event.topic !== TOPIC || event.type !== EVT_RE_PAIRING_REQUIRED) return
    callback(event.payload as SetupRePairingRequiredEvent)
  })
}
