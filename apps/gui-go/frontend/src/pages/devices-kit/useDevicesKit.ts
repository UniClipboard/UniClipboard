import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from '@/components/ui/toast'
import type { DevicesKitSelection } from './devices-kit-model'
import { useDeviceInvitationRow } from './useDeviceInvitationRow'

/**
 * Manage Devices as the History window owns it: whether the sheet is open, its selected row (kept
 * between openings), the invitation row (which outlives the sheet) and the device that just joined.
 */
export function useDevicesKit(initiallyOpen = false) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(initiallyOpen)
  // Stays true until the sheet has animated out, so the device page (and its polling) lives only
  // while the sheet is on screen.
  const [mounted, setMounted] = useState(initiallyOpen)
  const [selection, setSelection] = useState<DevicesKitSelection>({ kind: 'local' })
  const [joinedPeerId, setJoinedPeerId] = useState<string | null>(null)
  const invitation = useDeviceInvitationRow(peerDeviceId => {
    // The row turns into the device that joined.
    setSelection({ kind: 'peer', id: peerDeviceId })
    setJoinedPeerId(peerDeviceId)
    if (!open) toast.success(t('devices.addDevice.success.title'))
  })
  const show = (next?: DevicesKitSelection) => {
    if (next) setSelection(next)
    setOpen(true)
    setMounted(true)
  }
  return {
    open,
    setOpen,
    mounted,
    /** The sheet finished closing; unmount it. */
    onClosed: () => setMounted(false),
    selection,
    setSelection,
    invitation,
    joinedPeerId,
    dismissJoined: () => setJoinedPeerId(null),
    /** Opens the sheet, on a given row when one is named. */
    show,
    /** Opens the sheet on the invitation, issuing it when there is none. */
    invite: () => {
      if (!invitation.active) invitation.start()
      show({ kind: 'invitation' })
    },
  }
}

export type DevicesKitController = ReturnType<typeof useDevicesKit>
