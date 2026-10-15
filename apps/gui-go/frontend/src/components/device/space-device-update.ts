import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import type { DeviceGroupChoicesFailure } from '@/api/daemon/device-group-choices-failure'
import type { SpaceDeviceUpdateStatusDto } from '@/api/generated/types.gen'
import { toast } from '@/components/ui/toast'

const loadFailureKeys: Record<DeviceGroupChoicesFailure, string> = {
  unavailable: 'devices.spaceDeviceUpdate.unavailable',
  unlock_required: 'devices.spaceDeviceUpdate.unlockRequired',
  recovery_required: 'devices.spaceDeviceUpdate.recoveryRequired',
}

export interface SpaceDeviceUpdatePresentation {
  kind: 'load_failure' | 'needs_attention' | 'in_progress'
  titleKey: string
  descriptionKey: string
  /** Querying again can help (a recovery requirement cannot be resolved by retrying). */
  canRetry: boolean
}

/** What the device list says about the space's device update, or `null` when there is nothing. */
export function presentSpaceDeviceUpdate(
  status: SpaceDeviceUpdateStatusDto | null | undefined,
  loadFailure: DeviceGroupChoicesFailure | null
): SpaceDeviceUpdatePresentation | null {
  if (loadFailure) {
    const keyPrefix = loadFailureKeys[loadFailure]
    return {
      kind: 'load_failure',
      titleKey: `${keyPrefix}.title`,
      descriptionKey: `${keyPrefix}.description`,
      canRetry: loadFailure !== 'recovery_required',
    }
  }
  if (!status || status.phase === 'completed') return null
  if (status.phase === 'needs_attention') {
    return {
      kind: 'needs_attention',
      titleKey: 'devices.spaceDeviceUpdate.attention.title',
      descriptionKey: status.reason
        ? `devices.spaceDeviceUpdate.attention.reasons.${status.reason}`
        : 'devices.spaceDeviceUpdate.attention.description',
      canRetry: false,
    }
  }
  const retrying = status.phase === 'retryable_failure'
  return {
    kind: 'in_progress',
    titleKey: retrying
      ? 'devices.spaceDeviceUpdate.retrying.title'
      : 'devices.spaceDeviceUpdate.updating.title',
    descriptionKey: retrying
      ? 'devices.spaceDeviceUpdate.retrying.description'
      : 'devices.spaceDeviceUpdate.updating.description',
    canRetry: false,
  }
}

/** Announces once that a running space device update finished. */
export function useSpaceDeviceUpdateCompletedToast(
  phase: SpaceDeviceUpdateStatusDto['phase'] | undefined
) {
  const { t } = useTranslation()
  const previousPhase = useRef(phase)
  useEffect(() => {
    if (previousPhase.current && previousPhase.current !== 'completed' && phase === 'completed') {
      toast.success(t('devices.spaceDeviceUpdate.completed'), {
        id: 'space-device-update-completed',
      })
    }
    previousPhase.current = phase
  }, [phase, t])
}
