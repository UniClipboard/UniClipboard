import type { DeviceGroupChoices } from '@/api/daemon/device-trust'
import { DeviceTrustDecisionContent } from '@/components/device/DeviceTrustDecisionContent'
import { Dialog, DialogContent } from '@/components/ui/dialog'

export function DeviceTrustDialog({
  deviceGroups,
  busy,
  error,
  localRemovalConfirmationIssueId = null,
  onChoose,
}: {
  deviceGroups: DeviceGroupChoices
  busy: boolean
  error: string | null
  localRemovalConfirmationIssueId?: string | null
  onChoose: (issueId: string, choiceId: string, confirmLocalRemoval: boolean) => void
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
        className="bg-card text-card-foreground sm:max-w-xl"
        showCloseButton={false}
      >
        <DeviceTrustDecisionContent
          key={issueId}
          deviceGroups={deviceGroups}
          busy={busy}
          error={error}
          localRemovalConfirmationIssueId={localRemovalConfirmationIssueId}
          onChoose={onChoose}
        />
      </DialogContent>
    </Dialog>
  )
}
