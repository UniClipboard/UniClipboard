import { decisionFingerprint } from '@/components/device/device-group-presentation'
import { DeviceTrustDecisionContent } from '@/components/device/DeviceTrustDecisionContent'
import { DeviceTrustDecisionResult } from '@/components/device/DeviceTrustDecisionResult'
import { Dialog, DialogContent } from '@/components/ui/dialog'
import { useDeviceTrust } from '@/hooks/useDeviceTrust'
import { useDeviceTrustDesktopEffects } from '@/hooks/useDeviceTrustDesktopEffects'

export function DeviceTrustDialogHost() {
  const state = useDeviceTrust()
  useDeviceTrustDesktopEffects(state.snapshot)
  if (!state.deviceGroups?.issues.length && !state.decision) return null
  return (
    <Dialog open onOpenChange={(_open, details) => details.cancel()} disablePointerDismissal>
      <DialogContent
        showCloseButton={false}
        className="overflow-hidden sm:max-w-xl"
        data-testid="device-trust-dialog"
      >
        {state.decision ? (
          <DeviceTrustDecisionResult
            decision={state.decision}
            current={state.deviceGroups}
            loading={state.loading || state.decisionBusy}
            error={state.decisionError}
            onRefresh={() => void state.refresh()}
            onContinue={() => void state.acknowledgeDecision()}
          />
        ) : state.deviceGroups?.issues.length ? (
          <DeviceTrustDecisionContent
            key={`${decisionFingerprint(state.deviceGroups.issues[0])}:${state.decisionError === 'device_state_changed'}`}
            deviceGroups={state.deviceGroups}
            busy={state.decisionBusy}
            error={state.decisionError}
            localRemovalConfirmationIssueId={state.localRemovalConfirmationIssueId}
            confirmationChoiceId={
              state.localRemovalConfirmationIssueId ? state.localRemovalConfirmationChoiceId : null
            }
            onRefresh={() => void state.refresh()}
            onBack={state.cancelLocalConfirmation}
            onChoose={(...args) => void state.choose(...args)}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
