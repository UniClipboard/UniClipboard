import { useTranslation } from 'react-i18next'
import type { DeviceGroupChoicesFailure } from '@/api/daemon/device-group-choices-failure'
import type { SpaceDeviceUpdateStatusDto } from '@/api/generated/types.gen'
import {
  presentSpaceDeviceUpdate,
  useSpaceDeviceUpdateCompletedToast,
} from '@/components/device/space-device-update'
import { InlineNotice } from '@/ui-kit'

/** The space's device update under Paired devices: running, retrying, needing attention or unreadable. */
export function DevicesKitSpaceUpdate({
  status,
  loadFailure,
  onRetry,
}: {
  status?: SpaceDeviceUpdateStatusDto | null
  loadFailure: DeviceGroupChoicesFailure | null
  onRetry: () => void
}) {
  const { t } = useTranslation()
  useSpaceDeviceUpdateCompletedToast(status?.phase)
  const presentation = presentSpaceDeviceUpdate(status, loadFailure)
  if (!presentation) return null
  return (
    <div data-testid="space-device-update-status" data-phase={status?.phase}>
      <InlineNotice
        tone={
          presentation.kind === 'load_failure'
            ? 'error'
            : presentation.kind === 'needs_attention'
              ? 'warning'
              : 'info'
        }
        title={t(presentation.titleKey)}
        text={t(presentation.descriptionKey)}
        action={
          presentation.canRetry
            ? { label: t('devices.list.actions.retry'), onClick: onRetry }
            : undefined
        }
      />
    </div>
  )
}
