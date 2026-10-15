import { useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import type { ContentTypes } from '@/api/daemon/member'
import { DEFAULT_SEND_CONTENT_TYPES } from '@/api/daemon/member'
import { toast } from '@/components/ui/toast'
import { subscribeDeviceSyncChanged } from '@/lib/device-sync-events'
import { createLogger } from '@/lib/logger'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  fetchMemberSyncPreferences,
  updateMemberSyncPreferences,
} from '@/store/slices/devicesSlice'

const log = createLogger('peer-sync-preferences')

/**
 * One peer's sync preferences: the stored value, its protection status and the three edits the
 * detail offers (sync on/off, one content type's direction, restore defaults). Edits are applied
 * optimistically by the slice; a failure reports itself and re-fetches the authoritative value.
 */
export function usePeerSyncPreferences(deviceId: string) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()

  const preferences = useAppSelector(state => state.devices.memberSyncPreferences[deviceId])
  const sendEnabled = preferences?.sendEnabled ?? true
  const receiveEnabled = preferences?.receiveEnabled ?? true
  const protectionStatus = useAppSelector(
    state =>
      state.devices.spaceProtection?.members.find(member => member.deviceId === deviceId)?.status
  )

  useEffect(() => {
    if (deviceId) dispatch(fetchMemberSyncPreferences(deviceId))
  }, [dispatch, deviceId])

  useEffect(() => {
    return subscribeDeviceSyncChanged(deviceId, () => {
      dispatch(fetchMemberSyncPreferences(deviceId))
    })
  }, [dispatch, deviceId])

  const handleFailure = useCallback(
    (err: unknown, message: string, fields: Record<string, unknown> = {}) => {
      log.error({ err, ...fields }, message)
      toast.error(t('devices.settings.sync.updateFailed'))
      dispatch(fetchMemberSyncPreferences(deviceId))
    },
    [dispatch, deviceId, t]
  )

  const setSyncEnabled = useCallback(
    (checked: boolean) => {
      dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch: { sendEnabled: checked, receiveEnabled: checked },
        })
      )
        .unwrap()
        .catch(err => handleFailure(err, 'failed to update device sync preference'))
    },
    [dispatch, deviceId, handleFailure]
  )

  const setContentDirection = useCallback(
    (field: keyof ContentTypes, send: boolean, receive: boolean) => {
      const patch = {
        sendContentTypes: { [field]: send },
        receiveContentTypes: { [field]: receive },
        ...(!sendEnabled && send ? { sendEnabled: true } : {}),
        ...(!receiveEnabled && receive ? { receiveEnabled: true } : {}),
      }
      dispatch(updateMemberSyncPreferences({ deviceId, patch }))
        .unwrap()
        .catch(err => handleFailure(err, 'failed to update content sync direction', { field }))
    },
    [dispatch, deviceId, handleFailure, receiveEnabled, sendEnabled]
  )

  const restoreDefaults = useCallback(async () => {
    try {
      await dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch: {
            sendEnabled: true,
            receiveEnabled: true,
            sendContentTypes: DEFAULT_SEND_CONTENT_TYPES,
            receiveContentTypes: DEFAULT_SEND_CONTENT_TYPES,
          },
        })
      ).unwrap()
      dispatch(fetchMemberSyncPreferences(deviceId))
    } catch (err) {
      handleFailure(err, 'failed to restore default sync preferences')
    }
  }, [dispatch, deviceId, handleFailure])

  return {
    preferences,
    sendEnabled,
    receiveEnabled,
    protectionStatus,
    setSyncEnabled,
    setContentDirection,
    restoreDefaults,
  }
}
