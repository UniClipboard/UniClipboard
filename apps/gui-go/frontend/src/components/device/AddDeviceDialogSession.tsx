import DeviceInvitationFlow from '@/components/device/DeviceInvitationFlow'
import { Dialog, DialogContent } from '@/components/ui/dialog'
import { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

export interface AddDeviceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSuccess?: () => void
}

export default function AddDeviceDialogSession({
  onOpenChangeComplete,
  ...props
}: AddDeviceDialogProps & {
  onOpenChangeComplete: (open: boolean) => void
}) {
  const invitationState = useAddDeviceInvitation(props)
  const busy = invitationState.loading || invitationState.passphraseChangeSubmitting
  const handleOpenChange = (nextOpen: boolean) => {
    if (!busy) props.onOpenChange(nextOpen)
  }
  return (
    <Dialog
      open={props.open}
      onOpenChange={handleOpenChange}
      onOpenChangeComplete={onOpenChangeComplete}
      disablePointerDismissal
    >
      <DialogContent className="sm:max-w-md" showCloseButton={!busy}>
        <DeviceInvitationFlow invitationState={invitationState} onOpenChange={handleOpenChange} />
      </DialogContent>
    </Dialog>
  )
}
