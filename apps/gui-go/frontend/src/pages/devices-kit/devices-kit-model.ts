import type { TFunction } from 'i18next'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import type { StatusDotTone } from '@/components/device/StatusDot'
import type { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'
import { formatInvitationRemaining } from '@/lib/invitation-code'
import type { IconName, StatusTone } from '@/ui-kit'

/** The selected row of Manage Devices: this device, a member, a removed device, a phone or the invitation. */
export type DevicesKitSelection =
  | { kind: 'local' }
  | { kind: 'peer'; id: string }
  | { kind: 'removed'; id: string }
  | {
      kind: 'mobile'
      id: string
      /** One-time credentials of a phone just added, shown once in its detail. */
      pendingCredential?: RegisterMobileDeviceResult
    }
  | { kind: 'invitation' }

export const INVITATION_ROW_ID = 'invitation'

/** Router state that opens Manage Devices with the History window (`/devices` in a kit build). */
export const MANAGE_DEVICES_STATE = 'manageDevices'

export function selectionId(selection: DevicesKitSelection): string {
  return 'id' in selection ? `${selection.kind}:${selection.id}` : selection.kind
}

export function parseSelectionId(id: string): DevicesKitSelection {
  if (id === 'local' || id === INVITATION_ROW_ID) return { kind: id }
  const at = id.indexOf(':')
  const kind = id.slice(0, at)
  const rest = id.slice(at + 1)
  if (kind === 'peer' || kind === 'removed' || kind === 'mobile') return { kind, id: rest }
  return { kind: 'local' }
}

/** The kit's status tone for the app's dot tones. */
export const dotTone: Record<StatusDotTone, StatusTone> = {
  success: 'positive',
  info: 'info',
  warning: 'warning',
  off: 'neutral',
}

/** Phones by their name, computers otherwise (the daemon does not report a device kind). */
export function deviceIcon(name: string | null | undefined): IconName {
  const lower = name?.toLowerCase() ?? ''
  return lower.includes('iphone') ||
    lower.includes('phone') ||
    lower.includes('android') ||
    lower.includes('pixel')
    ? 'phone'
    : 'desktop'
}

type InvitationState = ReturnType<typeof useAddDeviceInvitation>

/** What the invitation row and the detail's status line say about the invitation now. */
export function invitationSummary(
  state: InvitationState,
  t: TFunction
): { text: string; tone: StatusTone } {
  switch (state.step) {
    case 'credentials':
    case 'reset_passphrase':
      return { text: t('devices.addDevice.rePairing.title'), tone: 'warning' }
    case 'failed':
      return { text: t('devices.addDevice.failed.title'), tone: 'critical' }
    case 'success':
      return { text: t('devices.addDevice.success.title'), tone: 'positive' }
    case 'invitation':
      if (state.error && !state.invitation) return { text: state.error, tone: 'critical' }
      if (!state.invitation) return { text: t('devices.addDevice.loading'), tone: 'neutral' }
      if (state.expired) return { text: t('devices.addDevice.expired'), tone: 'neutral' }
      return {
        text: t('devices.addDevice.expiresIn', {
          remaining: formatInvitationRemaining(state.remaining),
        }),
        tone: 'warning',
      }
  }
}
