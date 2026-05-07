/**
 * SpaceMembersPanel —— P2P 加密空间成员 panel(方案 F · macOS-native list)。
 *
 * 设计语言与 MobileShortcutDevicesPanel 完全一致:
 *
 *   ┌─ Section header ──────────────────────────────────────────┐
 *   │ Paired devices                              [+ Add device] │
 *   │ 2 paired · 1 online                                        │
 *   ├─ List container ────────────────────────────────────────────┤
 *   │ 💻 Mark's Windows               Online · Direct          ⌄ │
 *   │ 💻 Office MacBook               Offline · Out of LAN     ⌄ │
 *   └─────────────────────────────────────────────────────────────┘
 *
 * 取消旧实现的 ALL CAPS 标题、5 色循环 icon、2 列网格、虚线加号大卡片。
 * 单色 icon(在线 = primary,离线 = muted)配合 inline status 文字传达层级,
 * 而不是用 5 种饱和色制造视觉噪音。
 *
 * 注意:所有 dialog (AddDeviceDialog / DeviceSettingsSheet / UnpairAlertDialog)
 * 在文件末尾统一根级渲染,不要放进任何条件分支里 —— 配对成功时
 * spaceMembers 从 0 变 1 会让分支切换,分支内的 dialog 会被 React 视作不同
 * 位置卸载并重建,AddDeviceDialog 的 step='success' 状态会丢失,
 * 重建后的实例又会再次拉取新邀请码。
 */

import { AlertTriangle, ChevronRight, Plus, RefreshCw } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import AddDeviceDialog from './AddDeviceDialog'
import { deriveBadgeKind } from './ConnectionChannelBadge'
import { getDeviceIcon } from './device-utils'
import DeviceSettingsSheet from './DeviceSettingsSheet'
import UnpairAlertDialog from './UnpairAlertDialog'
import { unpairDevice } from '@/api/daemon/members'
import type { ConnectionChannel } from '@/api/daemon/members'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { useSetting } from '@/hooks/useSetting'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { fetchSpaceMembers, clearSpaceMembersError } from '@/store/slices/devicesSlice'

const log = createLogger('space-members-panel')

const SpaceMembersPanel: React.FC = () => {
  const { t } = useTranslation()
  const { setting } = useSetting()
  const navigate = useNavigate()
  const dispatch = useAppDispatch()
  const {
    spaceMembers: rawSpaceMembers,
    spaceMembersError,
    localDevice,
  } = useAppSelector(state => state.devices)
  const spaceMembers = localDevice
    ? rawSpaceMembers.filter(d => d.peerId !== localDevice.peerId)
    : rawSpaceMembers
  const globalAutoSyncOff = setting?.sync.autoSync === false
  const globalFileSyncOff = setting?.fileSync?.fileSyncEnabled === false
  const lanOnlyActive = setting?.network?.allowRelayFallback === false

  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null)
  const [sheetOpen, setSheetOpen] = useState(false)
  const [unpairDialogOpen, setUnpairDialogOpen] = useState(false)
  const [unpairTargetId, setUnpairTargetId] = useState<string | null>(null)
  const [addDialogOpen, setAddDialogOpen] = useState(false)

  useEffect(() => {
    dispatch(fetchSpaceMembers())

    const handler = (event: { topic: string; eventType: string; payload: unknown }) => {
      if (event.topic !== 'peers') return
      if (event.eventType === 'peers.changed') {
        dispatch(fetchSpaceMembers())
      }
    }
    const unsub = daemonWs.subscribe(['peers'], handler)
    return unsub
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
      await unpairDevice(unpairTargetId)
      dispatch(fetchSpaceMembers())
      setUnpairDialogOpen(false)
      setSheetOpen(false)
      setUnpairTargetId(null)
    } catch (error) {
      log.error({ err: error }, 'Failed to unpair device')
    }
  }

  const handleRetry = () => {
    dispatch(clearSpaceMembersError())
    dispatch(fetchSpaceMembers())
  }

  const selectedDevice = spaceMembers.find(d => d.peerId === selectedDeviceId)
  const unpairTargetDevice = spaceMembers.find(d => d.peerId === unpairTargetId)
  const onlineCount = spaceMembers.filter(d => d.connected).length

  // ── Body ─────────────────────────────────────────────────────────────
  let body: React.ReactNode
  if (spaceMembersError) {
    body = (
      <Alert variant="destructive">
        <AlertDescription className="flex items-center gap-3">
          <span className="flex-1">{spaceMembersError}</span>
          <Button variant="ghost" size="icon-sm" onClick={handleRetry}>
            <RefreshCw className="h-4 w-4" />
          </Button>
        </AlertDescription>
      </Alert>
    )
  } else {
    body = (
      <div className="overflow-hidden rounded-xl border border-border/60 bg-card">
        {spaceMembers.length === 0 ? (
          <EmptyRow t={t} />
        ) : (
          <ul className="divide-y divide-border/50">
            {spaceMembers.map(device => (
              <DeviceRow
                key={device.peerId}
                deviceName={device.deviceName}
                connected={device.connected}
                channel={device.channel ?? 'unknown'}
                lanOnlyActive={lanOnlyActive}
                onClick={() => openSheet(device.peerId)}
              />
            ))}
          </ul>
        )}
      </div>
    )
  }

  return (
    <>
      <section className="space-y-3">
        {/* ── 同步暂停告警 ───────────────────────────────────────── */}
        {globalAutoSyncOff && spaceMembers.length > 0 && (
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

        {/* ── Section header ─────────────────────────────────────── */}
        <div className="mb-2 flex items-end justify-between gap-3 px-1">
          <div className="min-w-0">
            <h3 className="text-base font-semibold text-foreground">
              {t('devices.pairedDevices.title')}
            </h3>
            <p className="mt-0.5 text-xs leading-tight text-muted-foreground">
              {spaceMembers.length === 0
                ? t('devices.pairedDevices.subtitleEmpty')
                : t('devices.pairedDevices.subtitle', {
                    total: spaceMembers.length,
                    online: onlineCount,
                  })}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setAddDialogOpen(true)}
            className="shrink-0"
          >
            <Plus className="h-3.5 w-3.5" />
            {t('devices.list.actions.addDevice')}
          </Button>
        </div>

        {body}
      </section>

      <AddDeviceDialog open={addDialogOpen} onOpenChange={setAddDialogOpen} />
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

// ─── Subcomponents ─────────────────────────────────────────────────────

interface DeviceRowProps {
  deviceName: string
  connected: boolean
  channel: ConnectionChannel
  lanOnlyActive: boolean
  onClick: () => void
}

const DeviceRow: React.FC<DeviceRowProps> = ({
  deviceName,
  connected,
  channel,
  lanOnlyActive,
  onClick,
}) => {
  const { t } = useTranslation()
  const Icon = getDeviceIcon(deviceName)
  const channelKind = deriveBadgeKind(channel, lanOnlyActive)

  const statusText = connected ? t('devices.list.status.online') : t('devices.list.status.offline')
  const channelLabel = t(`devices.list.channel.${channelKind}`)

  const iconBg = connected ? 'bg-primary/10 text-primary' : 'bg-muted/60 text-muted-foreground'

  return (
    <li>
      <button
        type="button"
        onClick={onClick}
        className="group flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-muted/30 focus-visible:bg-muted/30 focus-visible:outline-none"
      >
        <div
          className={`relative flex h-10 w-10 shrink-0 items-center justify-center rounded-lg ${iconBg}`}
        >
          {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon component reference keyed on deviceName, not a freshly-created component */}
          <Icon className="h-5 w-5" />
          {connected && (
            <span className="absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full bg-emerald-500 ring-2 ring-card" />
          )}
        </div>

        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium text-foreground">
            {deviceName || t('devices.list.labels.unknownDevice')}
          </p>
          <p className="mt-0.5 truncate text-xs text-muted-foreground">
            <span
              className={
                connected ? 'text-emerald-600 dark:text-emerald-400' : 'text-muted-foreground'
              }
            >
              {statusText}
            </span>
            <span className="mx-1.5 text-muted-foreground/50">·</span>
            <span>{channelLabel}</span>
          </p>
        </div>

        <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground/50 transition-colors group-hover:text-muted-foreground" />
      </button>
    </li>
  )
}

interface EmptyRowProps {
  t: ReturnType<typeof useTranslation>['t']
}

const EmptyRow: React.FC<EmptyRowProps> = ({ t }) => (
  <div className="px-4 py-4 text-center text-xs text-muted-foreground">
    {t('devices.list.empty.title')}
  </div>
)

export default SpaceMembersPanel
