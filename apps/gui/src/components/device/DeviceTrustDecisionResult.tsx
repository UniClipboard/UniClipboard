import { CheckCircle2, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { DeviceGroupChoices } from '@/api/daemon/device-trust'
import { presentDeviceGroups } from '@/components/device/device-group-presentation'
import { Button } from '@/components/ui/button'
import {
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { DeviceGroupDecision } from '@/contexts/device-trust-context'

export function DeviceTrustDecisionResult({
  decision,
  current,
  loading,
  error,
  onRefresh,
  onContinue,
}: {
  decision: DeviceGroupDecision
  current: DeviceGroupChoices | null
  loading: boolean
  error: string | null
  onRefresh: () => void
  onContinue: () => void
}) {
  const { t } = useTranslation()
  const view = presentDeviceGroups(decision.groups, t)
  const selected = view.choices.find(choice => choice.id === decision.choiceId)
  const submitting = decision.outcome === 'submitting'
  const done =
    !submitting &&
    !loading &&
    !error &&
    current !== null &&
    !current.issues.some(issue => issue.issueId === decision.issueId)
  const count = current?.issues.length ?? 0
  const title = done
    ? 'completed'
    : submitting
      ? 'processing'
      : loading
        ? 'checking'
        : decision.outcome === 'pending'
          ? 'pending'
          : 'uncertain'
  return (
    <>
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2 ">
          {done ? (
            <CheckCircle2 className="size-5 text-emerald-500" />
          ) : (
            (loading || submitting) && <Loader2 className="size-5 animate-spin" />
          )}
          {t(`deviceTrust.presentation.${title}`)}
        </DialogTitle>
        <DialogDescription>{selected?.title}</DialogDescription>
      </DialogHeader>
      <DialogBody className="space-y-3 py-2 text-ui-body [overflow-wrap:anywhere]">
        <p className="text-muted-foreground">{t('deviceTrust.presentation.members')}</p>
        <p>{selected?.members}</p>
        {selected?.paused && (
          <p className="text-destructive">
            {t('deviceTrust.presentation.paused', { names: selected.paused })}
          </p>
        )}
        {selected?.rejoin && (
          <p>{t('deviceTrust.presentation.rejoin', { names: selected.rejoin })}</p>
        )}
        {!done && decision.outcome === 'pending' && selected?.pending && (
          <p>{t('deviceTrust.presentation.waiting', { names: selected.pending })}</p>
        )}
        {decision.outcome === 're_pairing_required' && (
          <p>{t('deviceTrust.modal.rePairingRequired')}</p>
        )}
        {done && count > 0 && <p>{t('deviceTrust.presentation.more', { count })}</p>}
        {error && (
          <p role="alert" data-testid="device-trust-error" className="text-destructive">
            {t('deviceTrust.presentation.uncertain')}
          </p>
        )}
      </DialogBody>
      <DialogFooter>
        {done ? (
          <Button data-testid="device-trust-done" onClick={onContinue}>
            {t(`deviceTrust.presentation.${count ? 'next' : 'done'}`)}
          </Button>
        ) : (
          <Button
            data-testid="device-trust-recheck"
            disabled={submitting || loading}
            onClick={onRefresh}
          >
            {t('deviceTrust.presentation.retry')}
          </Button>
        )}
        {!done && !loading && !submitting && decision.outcome !== 'pending' && (
          <Button data-testid="device-trust-back" variant="outline" onClick={onContinue}>
            {t('deviceTrust.presentation.back')}
          </Button>
        )}
      </DialogFooter>
    </>
  )
}
