import { RefreshCw } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { getDeviceIcon } from './device-utils'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { formatPeerIdForDisplay } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { clearLocalDeviceError, fetchLocalDeviceInfo } from '@/store/slices/devicesSlice'

const ThisDeviceCard: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const { localDevice, localDeviceLoading, localDeviceError } = useAppSelector(
    state => state.devices
  )

  const handleRetry = () => {
    dispatch(clearLocalDeviceError())
    dispatch(fetchLocalDeviceInfo())
  }

  // Error state
  if (localDeviceError) {
    return (
      <SettingGroup title={t('devices.thisDevice.title')}>
        <div className="px-4 py-3">
          <Alert variant="destructive">
            <AlertDescription className="flex items-center gap-3">
              <span className="flex-1">{localDeviceError}</span>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={handleRetry}
                title={t('devices.list.actions.retry')}
              >
                <RefreshCw className="h-4 w-4" />
              </Button>
            </AlertDescription>
          </Alert>
        </div>
      </SettingGroup>
    )
  }

  // First-time loading: loading === true AND no cached device
  if (localDeviceLoading && localDevice === null) {
    return (
      <SettingGroup title={t('devices.thisDevice.title')}>
        <div className="flex items-center gap-4 px-4 py-3">
          <Skeleton className="h-10 w-10 rounded-lg" />
          <div className="flex flex-col gap-1.5">
            <Skeleton className="h-4 w-28" />
            <Skeleton className="h-3 w-20" />
          </div>
        </div>
      </SettingGroup>
    )
  }

  // Normal state (including background refresh — always show cached data)
  if (!localDevice) return null

  return (
    <SettingGroup title={t('devices.thisDevice.title')}>
      <div className="flex items-center gap-4 px-4 py-3">
        <div className="h-10 w-10 rounded-lg flex items-center justify-center ring-1 shadow-sm text-primary bg-primary/10 border-primary/20">
          {React.createElement(getDeviceIcon(localDevice.deviceName), { className: 'h-5 w-5' })}
        </div>
        <div className="flex flex-col gap-0.5 min-w-0">
          <span className="text-sm font-medium text-foreground truncate">
            {localDevice.deviceName}
          </span>
          <span className="text-xs text-muted-foreground font-mono">
            {formatPeerIdForDisplay(localDevice.peerId)}
          </span>
        </div>
      </div>
    </SettingGroup>
  )
}

export default ThisDeviceCard
