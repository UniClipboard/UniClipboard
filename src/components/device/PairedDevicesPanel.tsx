import { AlertTriangle, Monitor, MoreHorizontal, RefreshCw, Settings, Unlink } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { getDeviceIcon, getIconColor } from './device-utils'
import DeviceSettingsSheet from './DeviceSettingsSheet'
import UnpairAlertDialog from './UnpairAlertDialog'
import { onP2PPeerConnectionChanged, onP2PPeerNameUpdated, unpairP2PDevice } from '@/api/p2p'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useSetting } from '@/hooks/useSetting'
import { formatPeerIdForDisplay } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  fetchPairedDevices,
  clearPairedDevicesError,
  updatePeerConnectionStatus,
  updatePeerDeviceName,
} from '@/store/slices/devicesSlice'

const PairedDevicesPanel: React.FC = () => {
  const { t } = useTranslation()
  const { setting } = useSetting()
  const navigate = useNavigate()
  const dispatch = useAppDispatch()
  const { pairedDevices, pairedDevicesError } = useAppSelector(state => state.devices)
  const globalAutoSyncOff = setting?.sync.auto_sync === false
  const globalFileSyncOff = setting?.file_sync?.file_sync_enabled === false

  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null)
  const [sheetOpen, setSheetOpen] = useState(false)
  const [unpairDialogOpen, setUnpairDialogOpen] = useState(false)
  const [unpairTargetId, setUnpairTargetId] = useState<string | null>(null)

  useEffect(() => {
    dispatch(fetchPairedDevices())

    let unlistenConnection: (() => void) | undefined
    let unlistenName: (() => void) | undefined

    const setupConnectionListener = async () => {
      unlistenConnection = await onP2PPeerConnectionChanged(event => {
        dispatch(
          updatePeerConnectionStatus({
            peerId: event.peerId,
            connected: event.connected,
            deviceName: event.deviceName ?? undefined,
          })
        )
      })
    }

    const setupNameListener = async () => {
      unlistenName = await onP2PPeerNameUpdated(event => {
        dispatch(
          updatePeerDeviceName({
            peerId: event.peerId,
            deviceName: event.deviceName,
          })
        )
      })
    }

    setupConnectionListener()
    setupNameListener()

    return () => {
      unlistenConnection?.()
      unlistenName?.()
    }
  }, [dispatch])

  const openSheet = (peerId: string) => {
    setSelectedDeviceId(peerId)
    setSheetOpen(true)
  }

  const handleUnpairRequest = (peerId: string) => {
    setUnpairTargetId(peerId)
    setUnpairDialogOpen(true)
  }

  const handleUnpairConfirm = async () => {
    if (!unpairTargetId) return
    try {
      await unpairP2PDevice(unpairTargetId)
      dispatch(fetchPairedDevices())
      setUnpairDialogOpen(false)
      setSheetOpen(false)
      setUnpairTargetId(null)
    } catch (error) {
      console.error('Failed to unpair device:', error)
    }
  }

  const handleRetry = () => {
    dispatch(clearPairedDevicesError())
    dispatch(fetchPairedDevices())
  }

  const selectedDevice = pairedDevices.find(d => d.peerId === selectedDeviceId)
  const unpairTargetDevice = pairedDevices.find(d => d.peerId === unpairTargetId)

  if (pairedDevicesError) {
    return (
      <SettingGroup title={t('devices.pairedDevices.title')}>
        <div className="px-4 py-3">
          <Alert variant="destructive">
            <AlertDescription className="flex items-center gap-3">
              <span className="flex-1">{pairedDevicesError}</span>
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

  if (pairedDevices.length === 0) {
    return (
      <div className="mx-auto flex h-full w-full max-w-xl flex-col items-center justify-center text-center">
        <div className="mb-5 rounded-full bg-muted/30 p-5 ring-1 ring-border/50">
          <Monitor className="h-10 w-10 text-muted-foreground/50" />
        </div>
        <h3 className="mb-2 text-xl font-semibold text-foreground">
          {t('devices.list.empty.title')}
        </h3>
        <p className="max-w-sm text-muted-foreground">{t('devices.list.empty.description')}</p>
      </div>
    )
  }

  return (
    <>
      {globalAutoSyncOff && (
        <Alert className="border-amber-500/20 bg-amber-500/10">
          <AlertTriangle className="h-4 w-4 text-amber-500" />
          <AlertDescription className="text-amber-700 dark:text-amber-400">
            {t('devices.syncPaused.message')}{' '}
            <button
              type="button"
              onClick={() => navigate('/settings', { state: { category: 'sync' } })}
              className="font-medium underline hover:no-underline"
            >
              {t('devices.syncPaused.goToSettings')}
            </button>
          </AlertDescription>
        </Alert>
      )}

      <SettingGroup title={t('devices.pairedDevices.title')}>
        {pairedDevices.map((device, index) => {
          const Icon = getDeviceIcon(device.deviceName)
          const iconColor = getIconColor(index)

          return (
            <div
              key={device.peerId}
              className="flex items-center gap-3 px-4 py-3 hover:bg-accent/50 transition-colors cursor-pointer"
              onClick={() => openSheet(device.peerId)}
            >
              <button
                type="button"
                onClick={() => openSheet(device.peerId)}
                className="flex min-w-0 flex-1 items-center gap-4 text-left outline-none cursor-pointer"
              >
                <div
                  className={`h-10 w-10 shrink-0 rounded-lg flex items-center justify-center ring-1 shadow-sm ${iconColor}`}
                >
                  <Icon className="h-5 w-5" />
                </div>
                <div className="flex min-w-0 flex-col gap-0.5">
                  <span className="truncate font-medium text-foreground text-sm">
                    {device.deviceName || t('devices.list.labels.unknownDevice')}
                  </span>
                  <span className="text-xs text-muted-foreground font-mono">
                    {formatPeerIdForDisplay(device.peerId)}
                  </span>
                </div>
              </button>

              <Badge variant={device.connected ? 'default' : 'secondary'}>
                {device.connected
                  ? t('devices.list.status.online')
                  : t('devices.list.status.offline')}
              </Badge>

              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="ghost" size="icon-sm" onClick={e => e.stopPropagation()}>
                    <MoreHorizontal className="h-4 w-4" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem
                    onClick={e => {
                      e.stopPropagation()
                      openSheet(device.peerId)
                    }}
                  >
                    <Settings className="h-4 w-4" />
                    {t('devices.list.actions.settings')}
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    variant="destructive"
                    onClick={e => {
                      e.stopPropagation()
                      handleUnpairRequest(device.peerId)
                    }}
                  >
                    <Unlink className="h-4 w-4" />
                    {t('devices.list.actions.unpair')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          )
        })}
      </SettingGroup>

      <DeviceSettingsSheet
        open={sheetOpen}
        onOpenChange={setSheetOpen}
        deviceId={selectedDeviceId || ''}
        device={selectedDevice}
        globalAutoSyncOff={globalAutoSyncOff}
        globalFileSyncOff={globalFileSyncOff}
        onUnpair={handleUnpairRequest}
      />

      <UnpairAlertDialog
        open={unpairDialogOpen}
        onOpenChange={setUnpairDialogOpen}
        deviceName={unpairTargetDevice?.deviceName || t('devices.list.labels.unknownDevice')}
        onConfirm={handleUnpairConfirm}
      />
    </>
  )
}

export default PairedDevicesPanel
