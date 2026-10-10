/**
 * Setup pairing realtime event subscribers (Slice4 P3 T3.1+).
 *
 * Mirror of `uc-daemon-contract::api::dto::setup_events`. Each subscriber
 * filters the daemon WS stream for one event type and decodes its payload.
 *
 * Topic: `setup`
 * Events:
 *   - `setup.invitationIssued`     (sponsor)
 *   - `setup.pairingCompleted`     (both sides)
 *   - `setup.invitationRevoked`    (sponsor)
 */

import { onDaemonRealtimeEvent } from '@/api/realtime'

export interface SetupInvitationIssuedEvent {
  code: string
  expiresAtMs: number
}

export interface SetupPairingCompletedEvent {
  sponsorDeviceId: string
  /** `null` on early failures before the joiner identity is committed. */
  joinerDeviceId: string | null
  success: boolean
  /** Failure reason when `success === false`. `null` on success. */
  reason: string | null
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
const EVT_PAIRING_COMPLETED = 'setup.pairingCompleted'
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

export async function onSetupPairingCompleted(
  callback: (event: SetupPairingCompletedEvent) => void
): Promise<() => void> {
  return onDaemonRealtimeEvent(event => {
    if (event.topic !== TOPIC || event.type !== EVT_PAIRING_COMPLETED) return
    callback(event.payload as SetupPairingCompletedEvent)
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
