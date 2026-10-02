import type { AddDeviceInvitationStep } from '@/hooks/useAddDeviceInvitation'

const titleKeys: Record<AddDeviceInvitationStep, string> = {
  credentials: 'devices.addDevice.rePairing.title',
  reset_passphrase: 'passphraseChange.title',
  invitation: 'devices.addDevice.title',
  success: 'devices.addDevice.success.title',
  failed: 'devices.addDevice.failed.title',
}

const descriptionKeys: Partial<Record<AddDeviceInvitationStep, string>> = {
  credentials: 'devices.addDevice.rePairing.subtitle',
  reset_passphrase: 'passphraseChange.description',
  invitation: 'devices.addDevice.subtitle',
}

export function getInvitationTitleKey(step: AddDeviceInvitationStep): string {
  return titleKeys[step]
}

export function getInvitationDescriptionKey(step: AddDeviceInvitationStep): string | null {
  return descriptionKeys[step] ?? null
}
