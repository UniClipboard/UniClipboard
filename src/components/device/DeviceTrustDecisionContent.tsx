import { Loader2, ShieldAlert } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DeviceTrustChoice, DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { getPendingDecisionView } from '@/components/device/device-trust-model'
import { DeviceTrustChoiceCard } from '@/components/device/DeviceTrustChoiceCard'
import { Button } from '@/components/ui/button'
import {
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

export function DeviceTrustDecisionContent({
  snapshot,
  busy,
  error,
  onDecide,
}: {
  snapshot: DeviceTrustSnapshot
  busy: boolean
  error: string | null
  onDecide: (choice: DeviceTrustChoice, confirmLocalRemoval: boolean) => void
}) {
  const { t } = useTranslation()
  const view = getPendingDecisionView(snapshot)
  const change = snapshot.currentChange
  const choices = change?.allowedChoices ?? []
  const [selectedChoice, setSelectedChoice] = useState<DeviceTrustChoice>(
    () => choices[0] ?? 'apply_change'
  )
  if (!view || !change || choices.length === 0) return null
  const choice = choices.includes(selectedChoice) ? selectedChoice : choices[0]

  const names = (items: typeof view.targets) =>
    items.length > 0
      ? items.map(item => item.label).join(t('deviceTrust.listSeparator'))
      : t('deviceTrust.modal.noDevices')

  return (
    <>
      <DialogHeader className="flex-row items-start gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-md bg-destructive/10 text-destructive">
          <ShieldAlert className="size-5" aria-hidden="true" />
        </span>
        <span className="min-w-0">
          <DialogTitle>{t('deviceTrust.modal.title')}</DialogTitle>
          <DialogDescription className="mt-1">
            {view.includesLocalDevice
              ? t('deviceTrust.modal.localSummary', { proposer: view.proposer.label })
              : t('deviceTrust.modal.summary', {
                  proposer: view.proposer.label,
                  targets: names(view.targets),
                })}
          </DialogDescription>
        </span>
      </DialogHeader>
      <DialogBody className="space-y-4 py-1">
        <div className="grid min-w-0 gap-3" role="radiogroup">
          {choices.includes('apply_change') && (
            <DeviceTrustChoiceCard
              selected={choice === 'apply_change'}
              disabled={busy}
              onSelect={() => setSelectedChoice('apply_change')}
              title={t(
                view.includesLocalDevice
                  ? 'deviceTrust.modal.leaveTitle'
                  : 'deviceTrust.modal.applyTitle'
              )}
              continuesWith={names(view.apply.continuesWith)}
              stopsWith={names(view.apply.stopsWith)}
            />
          )}
          {choices.includes('keep_current_device_group') && (
            <DeviceTrustChoiceCard
              selected={choice === 'keep_current_device_group'}
              disabled={busy}
              onSelect={() => setSelectedChoice('keep_current_device_group')}
              title={t(
                view.includesLocalDevice
                  ? 'deviceTrust.modal.stayTitle'
                  : 'deviceTrust.modal.keepTitle'
              )}
              continuesWith={names(view.keepCurrent.continuesWith)}
              stopsWith={names(view.keepCurrent.stopsWith)}
            />
          )}
        </div>
        {error && (
          <p className="rounded-md border border-destructive/20 bg-destructive/10 px-3 py-2 text-sm font-medium text-destructive">
            {error === 'device_state_changed'
              ? t('deviceTrust.modal.stateChanged')
              : t('deviceTrust.modal.failed')}
          </p>
        )}
      </DialogBody>
      <DialogFooter>
        <Button
          className="min-w-24"
          disabled={busy}
          onClick={() => onDecide(choice, choice === 'apply_change' && view.includesLocalDevice)}
        >
          {busy && <Loader2 className="animate-spin" aria-hidden="true" />}
          {t('deviceTrust.actions.confirm')}
        </Button>
      </DialogFooter>
    </>
  )
}
