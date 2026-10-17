import { useState } from 'react'
import { useChangePassphraseForm } from '@/components/security/useChangePassphraseForm'
import { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

/**
 * The invitation shown as a row of Manage Devices and of the sidebar's Devices section. The daemon
 * owns at most one current invitation and closing a view does not revoke it, so the row lives as
 * long as the History window, not as long as the sheet: it stays (and keeps listening for the
 * joining device) after the sheet closes, until the device joins, it is cancelled or closed.
 */
export function useDeviceInvitationRow(onJoined: (peerDeviceId: string) => void) {
  const [active, setActive] = useState(false)
  const state = useAddDeviceInvitation({
    open: active,
    onOpenChange: setActive,
    onSuccess: peerDeviceId => {
      setActive(false)
      onJoined(peerDeviceId)
    },
  })
  const passphraseForm = useChangePassphraseForm({
    submitting: state.passphraseChangeSubmitting,
    onSubmittingChange: state.setPassphraseChangeSubmitting,
    onChanged: state.handlePassphraseChanged,
  })
  const busy = state.loading || state.passphraseChangeSubmitting
  return {
    busy,
    passphraseForm,
    /** The row exists. */
    active,
    state,
    /** Issues (or restores) the invitation and shows the row. */
    start: () => {
      passphraseForm.reset()
      setActive(true)
    },
    /** Removes the row without revoking the code (as closing the old dialog did). */
    close: () => {
      if (busy) return
      setActive(false)
      passphraseForm.reset()
    },
  }
}

export type DeviceInvitationRow = ReturnType<typeof useDeviceInvitationRow>
