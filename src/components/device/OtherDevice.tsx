import { motion, AnimatePresence } from 'framer-motion'
import { Smartphone, Monitor, Tablet, Trash2, Laptop, RefreshCw, ChevronRight } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import DeviceSettingsPanel from './DeviceSettingsPanel'
import { onP2PPeerConnectionChanged, onP2PPeerNameUpdated, unpairP2PDevice } from '@/api/p2p'
import { formatPeerIdForDisplay } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  fetchPairedDevices,
  clearPairedDevicesError,
  updatePeerConnectionStatus,
  updatePeerDeviceName,
} from '@/store/slices/devicesSlice'

const OtherDevice: React.FC = () => {
  const { t } = useTranslation()
  const [expandedDeviceId, setExpandedDeviceId] = useState<string | null>(null)
  const dispatch = useAppDispatch()
  const { pairedDevices, pairedDevicesLoading, pairedDevicesError } = useAppSelector(
    state => state.devices
  )

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

  const toggleDevice = (id: string) => {
    setExpandedDeviceId(prev => (prev === id ? null : id))
  }

  const handleUnpair = async (e: React.MouseEvent, peerId: string) => {
    e.stopPropagation()
    try {
      await unpairP2PDevice(peerId)
      dispatch(fetchPairedDevices())
      if (expandedDeviceId === peerId) {
        setExpandedDeviceId(null)
      }
    } catch (error) {
      console.error('Failed to unpair device:', error)
    }
  }

  const handleRetry = () => {
    dispatch(clearPairedDevicesError())
    dispatch(fetchPairedDevices())
  }

  const getDeviceIcon = (deviceName?: string | null) => {
    const name = deviceName?.toLowerCase() || ''
    if (name.includes('iphone') || name.includes('phone') || name.includes('android'))
      return Smartphone
    if (name.includes('ipad') || name.includes('tablet')) return Tablet
    if (
      name.includes('mac') ||
      name.includes('macbook') ||
      name.includes('pc') ||
      name.includes('windows')
    )
      return Laptop
    return Monitor
  }

  const getIconColor = (index: number) => {
    const colors = [
      'text-blue-500 bg-blue-500/10 border-blue-500/20',
      'text-purple-500 bg-purple-500/10 border-purple-500/20',
      'text-green-500 bg-green-500/10 border-green-500/20',
      'text-orange-500 bg-orange-500/10 border-orange-500/20',
      'text-primary bg-primary/10 border-primary/20',
    ]
    return colors[index % colors.length]
  }

  if (pairedDevicesLoading) {
    return (
      <div className="space-y-4">
        {[1, 2, 3].map(i => (
          <div key={i} className="border border-border/50 rounded-lg bg-card p-6">
            <div className="animate-pulse flex items-center gap-5">
              <div className="h-14 w-14 bg-muted rounded-md"></div>
              <div className="space-y-2 flex-1">
                <div className="h-5 bg-muted rounded w-32"></div>
                <div className="h-4 bg-muted rounded w-24"></div>
              </div>
            </div>
          </div>
        ))}
      </div>
    )
  }

  if (pairedDevicesError) {
    return (
      <div className="space-y-4">
        <div className="border border-destructive/50 rounded-lg bg-card p-6">
          <div className="flex items-center gap-3">
            <p className="text-sm text-destructive">{pairedDevicesError}</p>
            <button
              type="button"
              onClick={handleRetry}
              className="p-1.5 text-destructive hover:bg-destructive/10 rounded-lg transition-colors"
              title={t('devices.list.actions.retry')}
            >
              <RefreshCw className="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>
    )
  }

  if (pairedDevices.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-20 text-center">
        <div className="bg-muted/30 p-6 rounded-full mb-6 ring-1 ring-border/50">
          <Monitor className="h-12 w-12 text-muted-foreground/50" />
        </div>
        <h3 className="text-xl font-semibold text-foreground mb-2">
          {t('devices.list.empty.title')}
        </h3>
        <p className="text-muted-foreground max-w-xs">{t('devices.list.empty.description')}</p>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col rounded-xl border border-border/50 bg-card/50 overflow-hidden divide-y divide-border/50">
        {pairedDevices.map((device, index) => {
          const Icon = getDeviceIcon(device.deviceName)
          const isExpanded = expandedDeviceId === device.peerId
          const iconColor = getIconColor(index)

          return (
            <div key={device.peerId} className="flex flex-col bg-card/30">
              <div
                className={`
                  relative flex items-center p-4
                  hover:bg-accent/50 transition-colors duration-200
                  ${isExpanded ? 'bg-accent/50' : ''}
                `}
              >
                <button
                  type="button"
                  aria-expanded={isExpanded}
                  aria-controls={`device-settings-${device.peerId}`}
                  onClick={() => toggleDevice(device.peerId)}
                  className="flex min-w-0 flex-1 items-center justify-between gap-3 text-left outline-none"
                >
                  <div className="flex min-w-0 items-center gap-4">
                    <div
                      className={`h-10 w-10 rounded-lg flex items-center justify-center ring-1 shadow-sm ${iconColor}`}
                    >
                      <Icon className="h-5 w-5" />
                    </div>

                    <div className="flex min-w-0 flex-col gap-0.5">
                      <div className="flex items-center gap-2">
                        <span className="truncate font-medium text-foreground text-sm">
                          {device.deviceName || t('devices.list.labels.unknownDevice')}
                        </span>
                        {device.connected && (
                          <span className="flex h-2 w-2 rounded-full bg-green-500 animate-pulse" />
                        )}
                      </div>
                      <span className="text-xs text-muted-foreground font-mono">
                        {formatPeerIdForDisplay(device.peerId)}
                      </span>
                    </div>
                  </div>

                  <div className="flex items-center gap-3">
                    <div
                      className={`text-xs px-2 py-0.5 rounded-full border ${
                        device.connected
                          ? 'bg-green-500/10 text-green-600 border-green-500/20'
                          : 'bg-muted text-muted-foreground border-border'
                      }`}
                    >
                      {device.connected
                        ? t('devices.list.status.online')
                        : t('devices.list.status.offline')}
                    </div>

                    <ChevronRight
                      className={`h-4 w-4 text-muted-foreground transition-transform duration-200 ${
                        isExpanded ? 'rotate-90' : ''
                      }`}
                    />
                  </div>
                </button>

                <button
                  type="button"
                  onClick={e => handleUnpair(e, device.peerId)}
                  className="ml-2 p-2 text-muted-foreground hover:text-destructive hover:bg-destructive/10 rounded-lg transition-colors"
                  title={t('devices.list.actions.unpair')}
                >
                  <Trash2 className="h-4 w-4" />
                </button>
              </div>

              <AnimatePresence>
                {isExpanded && (
                  <motion.div
                    id={`device-settings-${device.peerId}`}
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: 'auto', opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    transition={{ duration: 0.2, ease: 'easeInOut' }}
                    className="overflow-hidden bg-accent/20"
                  >
                    <div className="p-4 border-t border-border/50">
                      <DeviceSettingsPanel
                        deviceId={device.peerId}
                        deviceName={device.deviceName || t('devices.list.labels.unknownDevice')}
                      />
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          )
        })}
      </div>
    </div>
  )
}

export default OtherDevice
