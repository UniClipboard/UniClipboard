import AddDeviceInvitationStep from '@/components/device/AddDeviceInvitationStep'
import { AddDeviceFailureStep, AddDeviceSuccessStep } from '@/components/device/AddDeviceStatusStep'
import RePairingCredentialsStep from '@/components/device/RePairingCredentialsStep'
import ChangePassphraseForm from '@/components/security/ChangePassphraseForm'
import type { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

export function AddDeviceDialogBody({
  invitationState,
  formId,
}: {
  invitationState: ReturnType<typeof useAddDeviceInvitation>
  formId?: string
}) {
  switch (invitationState.step) {
    case 'credentials':
      return <RePairingCredentialsStep invitationState={invitationState} formId={formId} />
    case 'reset_passphrase':
      return (
        <ChangePassphraseForm
          onCancel={invitationState.handleCancelPassphraseChange}
          onChanged={invitationState.handlePassphraseChanged}
        />
      )
    case 'success':
      return <AddDeviceSuccessStep />
    case 'failed':
      return <AddDeviceFailureStep message={invitationState.failureMessage} />
    case 'invitation':
      return (
        <AddDeviceInvitationStep
          invitationState={invitationState}
          hasExternalActions={Boolean(formId)}
        />
      )
  }
}
