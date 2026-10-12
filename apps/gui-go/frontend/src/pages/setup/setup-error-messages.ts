import type { InitializeSpaceErrorKind, RedeemInvitationErrorKind } from '@/api/daemon/setupV2'

type Translate = (key: string) => string

/** Message for a failed space initialization; `t` is scoped to `setup.initializeSpace`. */
export function initializeErrorMessage(
  t: Translate,
  kind: InitializeSpaceErrorKind | null
): string | null {
  switch (kind) {
    case null:
      return null
    case 'passphrase_mismatch':
      return t('errors.passphraseMismatch')
    case 'device_name_required':
      return t('errors.deviceNameRequired')
    case 'already_initialized':
    case 'already_setup':
      return t('errors.alreadyInitialized')
    case 'service_unavailable':
      return t('errors.serviceUnavailable')
    case 'internal':
    default:
      return t('errors.generic')
  }
}

/** Message for a failed invitation redemption; `t` is scoped to `setup.redeemInvitation`. */
export function redeemErrorMessage(
  t: Translate,
  kind: RedeemInvitationErrorKind | null
): string | null {
  switch (kind) {
    case null:
      return null
    case 'invitation_not_found':
      return t('errors.invitationNotFound')
    case 'invitation_expired':
      return t('errors.invitationExpired')
    case 'passphrase_mismatch':
      return t('errors.passphraseMismatch')
    case 'sponsor_unreachable':
      return t('errors.sponsorUnreachable')
    case 'sponsor_rejected':
      return t('errors.sponsorRejected')
    case 'sponsor_declined':
      return t('errors.sponsorDeclined')
    case 'sponsor_upgrade_required':
      return t('errors.sponsorUpgradeRequired')
    case 'timeout':
      return t('errors.timeout')
    case 'connection_lost':
      return t('errors.connectionLost')
    case 'service_unavailable':
      return t('errors.serviceUnavailable')
    case 'device_name_required':
      return t('errors.deviceNameRequired')
    case 'internal':
    default:
      return t('errors.generic')
  }
}
