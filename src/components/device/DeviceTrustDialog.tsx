import type { DeviceGroupChoices } from '@/api/daemon/device-trust'
import { decisionFingerprint } from '@/components/device/device-group-presentation'
import { DeviceTrustDecisionContent } from '@/components/device/DeviceTrustDecisionContent'
import { Dialog, DialogContent } from '@/components/ui/dialog'

export function DeviceTrustDialog({
  deviceGroups,
  busy,
  error,
  localRemovalConfirmationIssueId = null,
  onChoose,
  onRefresh,
  onBack,
}: {
  deviceGroups: DeviceGroupChoices
  busy: boolean
  error: string | null
  localRemovalConfirmationIssueId?: string | null
  onChoose: (issueId: string, choiceId: string, confirmLocalRemoval: boolean) => void
  onRefresh?: () => void
  onBack?: () => void
}) {
  const issueId = deviceGroups.issues[0]?.issueId
  if (!issueId) return null

  return (
    <Dialog
      open
      onOpenChange={(_open, eventDetails) => eventDetails.cancel()}
      disablePointerDismissal
    >
      <DialogContent
        data-testid="device-trust-dialog"
        className="overflow-hidden bg-card text-card-foreground sm:max-w-xl"
        showCloseButton={false}
      >
        <DeviceTrustDecisionContent
          key={`${decisionFingerprint(deviceGroups.issues[0])}:${error === 'device_state_changed'}`}
          deviceGroups={deviceGroups}
          busy={busy}
          error={error}
          localRemovalConfirmationIssueId={localRemovalConfirmationIssueId}
          onChoose={onChoose}
          onRefresh={onRefresh}
          onBack={onBack}
        />
      </DialogContent>
    </Dialog>
  )
}
