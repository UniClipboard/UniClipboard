import type { DeviceTrustChoice, DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { DeviceTrustDecisionContent } from '@/components/device/DeviceTrustDecisionContent'
import { Dialog, DialogContent } from '@/components/ui/dialog'

export function DeviceTrustDialog({
  snapshot,
  busy,
  error,
  localRemovalConfirmationChangeId = null,
  onDecide,
}: {
  snapshot: DeviceTrustSnapshot
  busy: boolean
  error: string | null
  localRemovalConfirmationChangeId?: string | null
  onDecide: (choice: DeviceTrustChoice, confirmLocalRemoval: boolean) => void
}) {
  const changeId = snapshot.currentChange?.changeId
  if (!changeId) return null

  return (
    <Dialog
      open
      onOpenChange={(_open, eventDetails) => eventDetails.cancel()}
      disablePointerDismissal
    >
      <DialogContent className="bg-card text-card-foreground sm:max-w-xl" showCloseButton={false}>
        <DeviceTrustDecisionContent
          key={changeId}
          snapshot={snapshot}
          busy={busy}
          error={error}
          localRemovalConfirmationChangeId={localRemovalConfirmationChangeId}
          onDecide={onDecide}
        />
      </DialogContent>
    </Dialog>
  )
}
