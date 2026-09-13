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
  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      onOpenChangeComplete={onOpenChangeComplete}
      disablePointerDismissal
    >
      <DialogContent className="sm:max-w-md">
        <DeviceInvitationFlow invitationState={invitationState} onOpenChange={props.onOpenChange} />
      </DialogContent>
    </Dialog>
  )
}
