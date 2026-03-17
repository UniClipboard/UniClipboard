import { AlertTriangle, Monitor, Plus, RefreshCw } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { getDeviceIcon, getIconColor } from './device-utils'
import DeviceSettingsSheet from './DeviceSettingsSheet'
import UnpairAlertDialog from './UnpairAlertDialog'
import { onP2PPeerDiscoveryChanged, onP2PPeerNameUpdated, unpairP2PDevice } from '@/api/p2p'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { useSetting } from '@/hooks/useSetting'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  fetchPairedDevices,
  clearPairedDevicesError,
  updatePeerPresenceStatus,
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
      unlistenConnection = await onP2PPeerDiscoveryChanged(event => {
        dispatch(
          updatePeerPresenceStatus({
            peerId: event.peerId,
            connected: event.discovered,
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
      <div className="space-y-2">
        <h3 className="text-xs font-medium text-muted-foreground px-1 uppercase tracking-wider">
          {t('devices.pairedDevices.title')}
        </h3>
        <div className="rounded-xl border border-border/60 bg-card p-4">
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
      </div>
    )
  }

  if (pairedDevices.length === 0) {
    return (
      <div className="space-y-2">
        <h3 className="text-xs font-medium text-muted-foreground px-1 uppercase tracking-wider">
          {t('devices.pairedDevices.title')}
        </h3>
        <div className="flex flex-col items-center rounded-xl border border-dashed border-border/80 py-10 px-6 text-center">
          <div className="mb-4 rounded-full bg-muted/50 p-4 ring-1 ring-border/50">
            <Monitor className="h-8 w-8 text-muted-foreground/70" />
          </div>
          <h3 className="mb-1.5 text-sm font-medium text-foreground">
            {t('devices.list.empty.title')}
          </h3>
          <p className="mb-4 max-w-xs text-xs text-muted-foreground">
            {t('devices.list.empty.description')}
          </p>
          <Button variant="outline" size="sm" disabled>
            <Plus className="h-3.5 w-3.5" />
            {t('devices.list.actions.addDevice')}
          </Button>
        </div>
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

      <div className="space-y-2">
        <h3 className="text-xs font-medium text-muted-foreground px-1 uppercase tracking-wider">
          {t('devices.pairedDevices.title')}
        </h3>
        <div className="grid grid-cols-2 gap-3">
          {pairedDevices.map((device, index) => {
            const Icon = getDeviceIcon(device.deviceName)
            const iconColor = getIconColor(index)

            return (
              <button
                key={device.peerId}
                type="button"
                onClick={() => openSheet(device.peerId)}
                className="group relative flex flex-col items-center rounded-2xl bg-card p-5 pt-6 pb-4 text-center shadow-sm ring-1 ring-border/40 transition-all hover:shadow-md hover:ring-border/60 cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                {/* Icon with status indicator */}
                <div className="relative mb-3">
                  <div
                    className={`h-14 w-14 rounded-2xl flex items-center justify-center shadow-sm ${iconColor}`}
                  >
                    <Icon className="h-7 w-7" />
                  </div>
                  {/* Online/offline dot */}
                  <span
                    className={`absolute -bottom-0.5 -right-0.5 h-3.5 w-3.5 rounded-full ring-2 ring-card ${
                      device.connected ? 'bg-emerald-500' : 'bg-muted-foreground/30'
                    }`}
                  />
                </div>

                {/* Device name */}
                <span className="truncate w-full font-medium text-foreground text-sm leading-tight">
                  {device.deviceName || t('devices.list.labels.unknownDevice')}
                </span>

                {/* Status text */}
                <span
                  className={`mt-1 text-xs ${
                    device.connected
                      ? 'text-emerald-600 dark:text-emerald-400'
                      : 'text-muted-foreground'
                  }`}
                >
                  {device.connected
                    ? t('devices.list.status.online')
                    : t('devices.list.status.offline')}
                </span>
              </button>
            )
          })}

          {/* Add device card (disabled, pending backend) */}
          <div
            title={t('devices.settings.badges.comingSoon')}
            className="flex flex-col items-center justify-center rounded-2xl border-2 border-dashed border-border/60 p-5 pt-6 pb-4 text-center opacity-50 cursor-not-allowed"
          >
            <div className="mb-3 h-14 w-14 rounded-2xl flex items-center justify-center bg-muted/50">
              <Plus className="h-7 w-7 text-muted-foreground/70" />
            </div>
            <span className="text-sm font-medium text-muted-foreground">
              {t('devices.list.actions.addDevice')}
            </span>
          </div>
        </div>
      </div>

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
