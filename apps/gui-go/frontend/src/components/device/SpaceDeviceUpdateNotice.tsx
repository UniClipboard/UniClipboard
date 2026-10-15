import { CircleAlert, LoaderCircle, RefreshCw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { DeviceGroupChoicesFailure } from '@/api/daemon/device-group-choices-failure'
import type { SpaceDeviceUpdateStatusDto } from '@/api/generated/types.gen'
import {
  presentSpaceDeviceUpdate,
  useSpaceDeviceUpdateCompletedToast,
} from '@/components/device/space-device-update'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

type Props = {
  status?: SpaceDeviceUpdateStatusDto | null
  loadFailure?: DeviceGroupChoicesFailure | null
  onRetry?: () => void
}

export function SpaceDeviceUpdateNotice({ status, loadFailure = null, onRetry }: Props) {
  const { t } = useTranslation()
  useSpaceDeviceUpdateCompletedToast(status?.phase)
  const presentation = presentSpaceDeviceUpdate(status, loadFailure)
  if (!presentation) return null

  if (presentation.kind === 'load_failure') {
    const canRetry = onRetry && presentation.canRetry
    return (
      <div
        data-testid="space-device-update-status"
        data-load-failure={loadFailure ?? undefined}
        className="mx-1 flex items-start gap-2.5 rounded-md bg-destructive/8 px-3 py-2.5 text-destructive"
      >
        <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <p className="text-ui-body font-medium">{t(presentation.titleKey)}</p>
          <p className="mt-0.5 text-ui-caption text-muted-foreground">
            {t(presentation.descriptionKey)}
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

  const needsAttention = presentation.kind === 'needs_attention'
  const Icon = needsAttention ? CircleAlert : LoaderCircle
  return (
    <div
      data-testid="space-device-update-status"
      data-phase={status?.phase}
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
        <p className="text-ui-body font-medium">{t(presentation.titleKey)}</p>
        <p className="mt-0.5 text-ui-caption text-muted-foreground">
          {t(presentation.descriptionKey)}
        </p>
      </div>
    </div>
  )
}
