import { useTranslation } from 'react-i18next'
import { AddDeviceDialogBody } from '@/components/device/AddDeviceDialogBody'
import {
  getInvitationDescriptionKey,
  getInvitationTitleKey,
} from '@/components/device/device-invitation-presentation'
import DeviceInvitationActions from '@/components/device/DeviceInvitationActions'
import { DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

interface AddDeviceDialogProps {
  invitationState: ReturnType<typeof useAddDeviceInvitation>
  onOpenChange: (open: boolean) => void
  showHeader?: boolean
}

export default function DeviceInvitationFlow({
  invitationState,
  onOpenChange,
  showHeader = true,
}: AddDeviceDialogProps) {
  const { t } = useTranslation()
  const { step } = invitationState
  const descriptionKey = getInvitationDescriptionKey(step)

  return (
    <div className="flex flex-col gap-4">
      {showHeader && (
        <DialogHeader>
          <DialogTitle>{t(getInvitationTitleKey(step))}</DialogTitle>
          {descriptionKey && <DialogDescription>{t(descriptionKey)}</DialogDescription>}
        </DialogHeader>
      )}

      <AddDeviceDialogBody invitationState={invitationState} />

      {step !== 'credentials' && step !== 'reset_passphrase' && step !== 'success' && (
        <DialogFooter>
          <DeviceInvitationActions invitationState={invitationState} onOpenChange={onOpenChange} />
        </DialogFooter>
      )}
    </div>
  )
}
