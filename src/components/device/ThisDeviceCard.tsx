import { RefreshCw } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { getDeviceIcon } from './device-utils'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useSetting } from '@/hooks/useSetting'
import { formatPeerIdForDisplay } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { clearLocalDeviceError, fetchLocalDeviceInfo } from '@/store/slices/devicesSlice'

const ThisDeviceCard: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const { localDevice, localDeviceLoading, localDeviceError, pairedDevices } = useAppSelector(
    state => state.devices
  )
  const { setting } = useSetting()
  const syncActive = setting?.sync.auto_sync !== false

  const handleRetry = () => {
    dispatch(clearLocalDeviceError())
    dispatch(fetchLocalDeviceInfo())
  }

  // Error state
  if (localDeviceError) {
    return (
      <div className="rounded-xl border border-border/60 bg-card p-5">
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
    )
  }

  // First-time loading
  if (localDeviceLoading && localDevice === null) {
    return (
      <div className="rounded-xl border border-border/60 bg-card p-5">
        <div className="flex items-center gap-4">
          <Skeleton className="h-14 w-14 rounded-xl" />
          <div className="flex flex-col gap-2">
            <Skeleton className="h-5 w-36" />
            <Skeleton className="h-3.5 w-24" />
          </div>
        </div>
        <div className="mt-4 flex gap-3">
          <Skeleton className="h-8 w-24 rounded-md" />
          <Skeleton className="h-8 w-24 rounded-md" />
        </div>
      </div>
    )
  }

  if (!localDevice) return null

  const onlineCount = pairedDevices.filter(d => d.connected).length
  const pairedCount = pairedDevices.length

  return (
    <div className="rounded-xl border border-border/60 bg-card p-5">
      <div className="flex items-center gap-4">
        <div className="h-14 w-14 shrink-0 rounded-xl flex items-center justify-center ring-1 shadow-sm text-emerald-500 bg-emerald-500/10 ring-emerald-500/20">
          {React.createElement(getDeviceIcon(localDevice.deviceName), {
            className: 'h-7 w-7',
          })}
        </div>
        <div className="flex flex-col gap-0.5 min-w-0">
          <span className="text-base font-semibold text-foreground truncate">
            {localDevice.deviceName}
          </span>
          <span className="text-xs text-muted-foreground font-mono">
            {formatPeerIdForDisplay(localDevice.peerId)}
          </span>
        </div>
      </div>

      {/* Stats row */}
      <div className="mt-4 flex items-center gap-3 text-xs text-muted-foreground">
        <span className="inline-flex items-center gap-1.5">
          <span
            className={`h-1.5 w-1.5 rounded-full ${syncActive ? 'bg-emerald-500' : 'bg-amber-500'}`}
          />
          {syncActive ? t('devices.thisDevice.syncActive') : t('devices.thisDevice.syncPaused')}
        </span>
        <span className="text-border">|</span>
        <span>{t('devices.thisDevice.pairedCount', { count: pairedCount })}</span>
        {pairedCount > 0 && (
          <>
            <span className="text-border">|</span>
            <span>{t('devices.thisDevice.onlineCount', { count: onlineCount })}</span>
          </>
        )}
      </div>
    </div>
  )
}

export default ThisDeviceCard
