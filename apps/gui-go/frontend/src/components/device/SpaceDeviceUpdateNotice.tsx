import { CircleAlert, LoaderCircle, RefreshCw } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import type { DeviceGroupChoicesFailure } from '@/api/daemon/device-group-choices-failure'
import type { SpaceDeviceUpdateStatusDto } from '@/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { toast } from '@/components/ui/toast'
import { cn } from '@/lib/utils'

type Props = {
  status?: SpaceDeviceUpdateStatusDto | null
  loadFailure?: DeviceGroupChoicesFailure | null
  onRetry?: () => void
}

const loadFailureKeys: Record<DeviceGroupChoicesFailure, string> = {
  unavailable: 'devices.spaceDeviceUpdate.unavailable',
  unlock_required: 'devices.spaceDeviceUpdate.unlockRequired',
  recovery_required: 'devices.spaceDeviceUpdate.recoveryRequired',
}

export function SpaceDeviceUpdateNotice({ status, loadFailure = null, onRetry }: Props) {
  const { t } = useTranslation()
  const previousPhase = useRef(status?.phase)

  useEffect(() => {
    if (
      previousPhase.current &&
      previousPhase.current !== 'completed' &&
      status?.phase === 'completed'
    ) {
      toast.success(t('devices.spaceDeviceUpdate.completed'), {
        id: 'space-device-update-completed',
      })
    }
    previousPhase.current = status?.phase
  }, [status?.phase, t])

  if (loadFailure) {
    const keyPrefix = loadFailureKeys[loadFailure]
    // Querying again cannot resolve a recovery requirement, so no retry is offered.
    const canRetry = onRetry && loadFailure !== 'recovery_required'
    return (
      <div
        data-testid="space-device-update-status"
        data-load-failure={loadFailure}
        className="mx-1 flex items-start gap-2.5 rounded-md bg-destructive/8 px-3 py-2.5 text-destructive"
      >
        <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <p className="text-ui-body font-medium">{t(`${keyPrefix}.title`)}</p>
          <p className="mt-0.5 text-ui-caption text-muted-foreground">
            {t(`${keyPrefix}.description`)}
          </p>
          {canRetry && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="mt-1.5 h-7 px-2"
              onClick={onRetry}
            >
              <RefreshCw />
              {t('devices.list.actions.retry')}
            </Button>
          )}
        </div>
      </div>
    )
  }

  if (!status || status.phase === 'completed') return null

  const needsAttention = status.phase === 'needs_attention'
  const titleKey = needsAttention
    ? 'devices.spaceDeviceUpdate.attention.title'
    : status.phase === 'retryable_failure'
      ? 'devices.spaceDeviceUpdate.retrying.title'
      : 'devices.spaceDeviceUpdate.updating.title'
  const descriptionKey = needsAttention
    ? status.reason
      ? `devices.spaceDeviceUpdate.attention.reasons.${status.reason}`
      : 'devices.spaceDeviceUpdate.attention.description'
    : status.phase === 'retryable_failure'
      ? 'devices.spaceDeviceUpdate.retrying.description'
      : 'devices.spaceDeviceUpdate.updating.description'
  const Icon = needsAttention ? CircleAlert : LoaderCircle

  return (
    <div
      data-testid="space-device-update-status"
      data-phase={status.phase}
      className={cn(
        'mx-1 flex items-start gap-2.5 rounded-md px-3 py-2.5',
        needsAttention ? 'bg-warning/10 text-warning' : 'bg-muted/45 text-foreground'
      )}
    >
      <Icon
        className={cn('mt-0.5 size-4 shrink-0', !needsAttention && 'motion-safe:animate-spin')}
        aria-hidden="true"
      />
      <div className="min-w-0">
        <p className="text-ui-body font-medium">{t(titleKey)}</p>
        <p className="mt-0.5 text-ui-caption text-muted-foreground">{t(descriptionKey)}</p>
      </div>
    </div>
  )
}
