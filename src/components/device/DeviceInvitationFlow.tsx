import { useTranslation } from 'react-i18next'
import { AddDeviceDialogBody } from '@/components/device/AddDeviceDialogBody'
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

  return (
    <div className="flex flex-col gap-4">
      {showHeader && (
        <DialogHeader>
          <DialogTitle>
            {step === 'credentials'
              ? t('devices.addDevice.rePairing.title')
              : step === 'success'
                ? t('devices.addDevice.success.title')
                : step === 'failed'
                  ? t('devices.addDevice.failed.title')
                  : t('devices.addDevice.title')}
          </DialogTitle>
          {(step === 'credentials' || step === 'invitation') && (
            <DialogDescription>
              {step === 'credentials'
                ? t('devices.addDevice.rePairing.subtitle')
                : t('devices.addDevice.subtitle')}
            </DialogDescription>
          )}
        </DialogHeader>
      )}

      <AddDeviceDialogBody invitationState={invitationState} />

      {step !== 'credentials' && step !== 'success' && (
        <DialogFooter>
          <DeviceInvitationActions invitationState={invitationState} onOpenChange={onOpenChange} />
        </DialogFooter>
      )}
    </div>
  )
}
