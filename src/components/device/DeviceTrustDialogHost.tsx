import { useTranslation } from 'react-i18next'
import { decisionFingerprint } from '@/components/device/device-group-presentation'
import { DeviceTrustDecisionContent } from '@/components/device/DeviceTrustDecisionContent'
import { DeviceTrustDecisionResult } from '@/components/device/DeviceTrustDecisionResult'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogTitle, DialogDescription } from '@/components/ui/dialog'
import { useDeviceTrust } from '@/hooks/useDeviceTrust'
import { useDeviceTrustDesktopEffects } from '@/hooks/useDeviceTrustDesktopEffects'

export function DeviceTrustDialogHost() {
  const { t } = useTranslation()
  const state = useDeviceTrust()
  useDeviceTrustDesktopEffects(state.snapshot)
  if (!state.deviceGroups?.issues.length && !state.decision && !state.decisionError) return null
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
        ) : (
          <>
            <DialogTitle>{t('deviceTrust.presentation.loading')}</DialogTitle>
            <DialogDescription
              data-testid={state.decisionError ? 'device-trust-error' : undefined}
              role={state.decisionError ? 'alert' : undefined}
            >
              {state.decisionError ? t('deviceTrust.presentation.loadFailed') : t('common.loading')}
            </DialogDescription>
            {state.decisionError && (
              <Button
                data-testid="device-trust-recheck"
                disabled={state.loading}
                onClick={() => void state.refresh()}
              >
                {t('deviceTrust.presentation.retry')}
              </Button>
            )}
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}
