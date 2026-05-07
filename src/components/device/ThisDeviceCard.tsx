import { ArrowRightLeft, RefreshCw } from 'lucide-react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getDeviceIcon } from './device-utils'
import SwitchSpaceDialog from './SwitchSpaceDialog'
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
  const { localDevice, localDeviceLoading, localDeviceError, spaceMembers } = useAppSelector(
    state => state.devices
  )
  const { setting } = useSetting()
  const syncActive = setting?.sync.autoSync !== false
  const [switchSpaceOpen, setSwitchSpaceOpen] = useState(false)

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

  const peers = spaceMembers.filter(d => d.peerId !== localDevice.peerId)
  const onlineCount = peers.filter(d => d.connected).length
  const pairedCount = peers.length

  return (
    <div className="rounded-xl border border-border/60 bg-card px-4 py-3">
      <div className="flex items-center gap-3">
        <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
          {React.createElement(getDeviceIcon(localDevice.deviceName), {
            className: 'h-5 w-5',
          })}
        </div>

        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="truncate text-sm font-medium text-foreground">
            {localDevice.deviceName}
          </span>
          <span className="truncate text-xs text-muted-foreground">
            <span
              className={
                syncActive
                  ? 'text-emerald-600 dark:text-emerald-400'
                  : 'text-amber-600 dark:text-amber-400'
              }
            >
              {syncActive ? t('devices.thisDevice.syncActive') : t('devices.thisDevice.syncPaused')}
            </span>
            <span className="mx-1.5 text-muted-foreground/50">·</span>
            <span>{t('devices.thisDevice.pairedCount', { count: pairedCount })}</span>
            {pairedCount > 0 && (
              <>
                <span className="mx-1.5 text-muted-foreground/50">·</span>
                <span>{t('devices.thisDevice.onlineCount', { count: onlineCount })}</span>
              </>
            )}
          </span>
          <span className="truncate font-mono text-[11px] text-muted-foreground/70">
            {formatPeerIdForDisplay(localDevice.peerId)}
          </span>
        </div>

        <Button
          variant="ghost"
          size="sm"
          className="shrink-0 text-xs text-muted-foreground hover:text-foreground"
          onClick={() => setSwitchSpaceOpen(true)}
        >
          <ArrowRightLeft className="mr-1.5 h-3.5 w-3.5" />
          {t('devices.switchSpace.button')}
        </Button>
      </div>

      <SwitchSpaceDialog open={switchSpaceOpen} onOpenChange={setSwitchSpaceOpen} />
    </div>
  )
}

export default ThisDeviceCard
