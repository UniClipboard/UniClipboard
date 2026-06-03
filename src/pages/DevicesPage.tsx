/**
 * DevicesPage —— "Hero + Tab + 卡片网格" 布局（参考 DevicesPagePrototypeV2）。
 *
 * 三层结构：
 *
 *   ┌─ Hero ──────────────────────────────────────────────────────┐
 *   │ 本设备图标 + 名字 + peerId + 切换空间 chip │ 配对 / 同步 pill │
 *   ├─ Tabs (P2P / Mobile) ───────────────────────────────────────┤
 *   │  ┌──────┐ ┌──────┐ ┌──────┐                                  │
 *   │  │ peer │ │ peer │ │  +   │                                  │
 *   │  └──────┘ └──────┘ └──────┘                                  │
 *   └─────────────────────────────────────────────────────────────┘
 *
 * 所有 dialog / modal 复用现成实现（AddDeviceDialog / DeviceSettingsDialog /
 * MobileSync* 系列），本页只重写视觉骨架与卡片排版。`refreshPresence`
 * 现在只在挂载和页面从隐藏切回可见时触发一次，常态依赖 daemon 主动推送
 * 的 `peers.changed` ws 事件驱动 UI（详见下方 useEffect 的注释）。
 */

import {
  ArrowRightLeft,
  Cable,
  CheckCircle2,
  ChevronRight,
  Pause,
  Plus,
  RefreshCw,
  Settings2,
  ShieldCheck,
  Smartphone,
  Wifi,
  WifiOff,
} from 'lucide-react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { refreshPresence } from '@/api/daemon'
import type { SpaceMember } from '@/api/daemon/members'
import { unpairDevice } from '@/api/daemon/members'
import {
  getMobileSyncSettings,
  isMobileSyncError,
  listMobileDevices,
  revokeMobileDevice,
  type MobileDeviceView,
  type MobileSyncError,
  type MobileSyncSettingsView,
  type RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import AddDeviceDialog from '@/components/device/AddDeviceDialog'
import AddMobileSyncDeviceDialog from '@/components/device/AddMobileSyncDeviceDialog'
import { deriveBadgeKind } from '@/components/device/connection-channel-utils'
import { getDeviceIcon } from '@/components/device/device-utils'
import DeviceSettingsDialog from '@/components/device/DeviceSettingsDialog'
import EnableMobileSyncDialog from '@/components/device/EnableMobileSyncDialog'
import MobileSyncCredentialModal from '@/components/device/MobileSyncCredentialModal'
import MobileSyncDeviceDialog from '@/components/device/MobileSyncDeviceDialog'
import MobileSyncSettingsDialog from '@/components/device/MobileSyncSettingsDialog'
import SwitchSpaceDialog from '@/components/device/SwitchSpaceDialog'
import UnpairAlertDialog from '@/components/device/UnpairAlertDialog'
import { Alert, AlertDescription } from '@/components/ui/alert'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Skeleton } from '@/components/ui/skeleton'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { formatPeerIdForDisplay } from '@/lib/utils'
import { cn } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  clearLocalDeviceError,
  clearSpaceMembersError,
  fetchLocalDeviceInfo,
  fetchSpaceMembers,
} from '@/store/slices/devicesSlice'

const log = createLogger('devices-page')

// ────────────────────────────────────────────────────────────────
// 顶层页面
// ────────────────────────────────────────────────────────────────

const DevicesPage: React.FC = () => {
  const dispatch = useAppDispatch()

  useEffect(() => {
    dispatch(fetchLocalDeviceInfo())
    dispatch(fetchSpaceMembers())

    // 上线感知由 daemon 的 PeerKeepAliveWorker 推送驱动：inbound presence
    // Online → outbound dial → peers.changed ws → 前端切亮（~1s）。
    // 离线感知交给 daemon 自身的 25s keepalive tick + QUIC idle watchdog。
    // 前端只在两个时刻主动拉一次 presence:
    //   1. 页面首次挂载 —— warm 一下 UI，避免 ws 推送之前显示陈旧状态。
    //   2. 标签页从隐藏切回可见 —— 用户回到前台时给一个兜底快照。
    // 不再 setInterval polling，关闭 Devices 页时 daemon 完全静默。
    const probe = () => {
      refreshPresence().catch(err => {
        // setup 未完成 / daemon 未就绪时 refresh_presence 会 5xx；
        // 不影响后续推送链路，warn 即可。
        log.warn({ err }, 'presence refresh failed')
      })
    }
    probe()

    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') {
        probe()
      }
    }
    document.addEventListener('visibilitychange', onVisibilityChange)
    return () => {
      document.removeEventListener('visibilitychange', onVisibilityChange)
    }
  }, [dispatch])

  return (
    <div className="flex flex-col h-full relative">
      <div className="flex-1 overflow-hidden relative">
        <ScrollArea className="h-full">
          <div className="space-y-8 px-6 pb-12 pt-6">
            <HeroSection />
            <DeviceTabs />
          </div>
        </ScrollArea>
      </div>
    </div>
  )
}

export default DevicesPage

// ────────────────────────────────────────────────────────────────
// Hero 区
// ────────────────────────────────────────────────────────────────

const HeroSection: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const { localDevice, localDeviceLoading, localDeviceError, spaceMembers } = useAppSelector(
    state => state.devices
  )
  const { setting } = useSetting()
  const syncActive = setting?.sync.autoSync !== false

  const peers = localDevice
    ? spaceMembers.filter(d => d.peerId !== localDevice.peerId)
    : spaceMembers
  const pairedCount = peers.length
  const onlineCount = peers.filter(p => p.connected).length

  if (localDeviceError) {
    return (
      <Alert variant="destructive">
        <AlertDescription className="flex items-center gap-3">
          <span className="flex-1">{localDeviceError}</span>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => {
              dispatch(clearLocalDeviceError())
              dispatch(fetchLocalDeviceInfo())
            }}
            title={t('devices.list.actions.retry')}
          >
            <RefreshCw className="h-4 w-4" />
          </Button>
        </AlertDescription>
      </Alert>
    )
  }

  if (localDeviceLoading && localDevice === null) {
    return (
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-[1fr_auto]">
        <div className="rounded-2xl border border-border/60 bg-card px-6 py-6">
          <div className="flex items-center gap-5">
            <Skeleton className="h-16 w-16 rounded-2xl" />
            <div className="flex flex-1 flex-col gap-2">
              <Skeleton className="h-3 w-12" />
              <Skeleton className="h-5 w-40" />
              <Skeleton className="h-3 w-32" />
            </div>
          </div>
        </div>
        <div className="grid grid-cols-2 gap-3 lg:flex lg:w-[220px] lg:flex-col">
          <Skeleton className="h-[68px] rounded-xl" />
          <Skeleton className="h-[68px] rounded-xl" />
        </div>
      </div>
    )
  }

  if (!localDevice) return null

  const Icon = getDeviceIcon(localDevice.deviceName)

  return (
    <div className="grid grid-cols-1 gap-3 lg:grid-cols-[1fr_auto]">
      {/* ── 本机 hero (左) ────────────────────────────────────── */}
      <div className="relative overflow-hidden rounded-2xl border border-border/60 bg-card px-6 py-6">
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 opacity-[0.04]"
          style={{
            backgroundImage: 'radial-gradient(circle at 1px 1px, currentColor 1px, transparent 0)',
            backgroundSize: '16px 16px',
          }}
        />
        <div className="relative flex items-center gap-5">
          <div className="flex h-16 w-16 shrink-0 items-center justify-center rounded-2xl bg-success/15 text-success shadow-sm ring-1 ring-success/20">
            {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon reference keyed on deviceName, not a freshly-created component */}
            <Icon className="h-8 w-8" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="text-[11px] uppercase tracking-wider text-muted-foreground">
                {t('devices.thisDevice.title')}
              </span>
              <span className="inline-flex h-1.5 w-1.5 rounded-full bg-success" />
            </div>
            <h2 className="mt-1 truncate text-xl font-semibold leading-tight text-foreground">
              {localDevice.deviceName}
            </h2>
            <p className="mt-1 truncate font-mono text-xs text-muted-foreground/80">
              {formatPeerIdForDisplay(localDevice.peerId)}
            </p>
          </div>

          <div className="flex shrink-0 items-start gap-2 self-start pt-1">
            <SpaceChip />
          </div>
        </div>
      </div>

      {/* ── 右侧 stat 列 ───────────────────────────────────────── */}
      <div className="grid grid-cols-2 gap-3 lg:flex lg:w-[220px] lg:flex-col">
        <StatPill
          icon={Wifi}
          label={t('devices.pairedDevices.title')}
          value={`${onlineCount}/${pairedCount}`}
          sublabel={
            pairedCount === 0
              ? t('devices.pairedDevices.subtitleEmpty')
              : t('devices.thisDevice.onlineCount', { count: onlineCount })
          }
          accent={onlineCount > 0 ? 'emerald' : 'muted'}
        />
        <StatPill
          icon={syncActive ? CheckCircle2 : Pause}
          label={t('devices.thisDevice.syncStatusLabel', { defaultValue: '同步状态' })}
          value={
            syncActive ? t('devices.thisDevice.syncActive') : t('devices.thisDevice.syncPaused')
          }
          sublabel={
            syncActive
              ? t('devices.thisDevice.syncRunningHint', { defaultValue: '正在自动同步' })
              : t('devices.syncPaused.goToSettings')
          }
          accent={syncActive ? 'emerald' : 'amber'}
        />
      </div>
    </div>
  )
}

const SpaceChip: React.FC = () => {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11px] font-medium text-muted-foreground ring-1 ring-border/60 transition-colors hover:bg-background hover:text-foreground hover:ring-border"
      >
        <ArrowRightLeft className="h-3 w-3" />
        <span>{t('devices.switchSpace.button')}</span>
      </button>
      <SwitchSpaceDialog open={open} onOpenChange={setOpen} />
    </>
  )
}

type StatAccent = 'emerald' | 'amber' | 'muted'

interface StatPillProps {
  icon: typeof Wifi
  label: string
  value: string
  sublabel: string
  accent: StatAccent
}

const STAT_ACCENT: Record<StatAccent, { icon: string; value: string }> = {
  emerald: {
    icon: 'bg-success/10 text-success',
    value: 'text-success',
  },
  amber: {
    icon: 'bg-warning/10 text-warning',
    value: 'text-warning',
  },
  muted: {
    icon: 'bg-muted text-muted-foreground',
    value: 'text-foreground',
  },
}

const StatPill: React.FC<StatPillProps> = ({ icon: Icon, label, value, sublabel, accent }) => {
  const a = STAT_ACCENT[accent]
  return (
    <div className="flex items-center gap-3 rounded-xl border border-border/60 bg-card px-4 py-3">
      <div className={cn('flex h-9 w-9 shrink-0 items-center justify-center rounded-lg', a.icon)}>
        <Icon className="h-4 w-4" />
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-[10px] uppercase tracking-wider text-muted-foreground">{label}</p>
        <p className={cn('truncate text-sm font-semibold', a.value)}>{value}</p>
        <p className="truncate text-[11px] text-muted-foreground/80">{sublabel}</p>
      </div>
    </div>
  )
}

// ────────────────────────────────────────────────────────────────
// 设备 tabs（P2P / Mobile）
// ────────────────────────────────────────────────────────────────

type TabKey = 'p2p' | 'mobile'

const DeviceTabs: React.FC = () => {
  const { t } = useTranslation()
  const [tab, setTab] = useState<TabKey>('p2p')

  // ── P2P 状态 ─────────────────────────────────────────────────
  const dispatch = useAppDispatch()
  const {
    spaceMembers: rawSpaceMembers,
    spaceMembersError,
    localDevice,
  } = useAppSelector(state => state.devices)
  const peers = localDevice
    ? rawSpaceMembers.filter(d => d.peerId !== localDevice.peerId)
    : rawSpaceMembers

  const [addP2PDialogOpen, setAddP2PDialogOpen] = useState(false)
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null)
  const [peerDialogOpen, setPeerDialogOpen] = useState(false)
  const [unpairDialogOpen, setUnpairDialogOpen] = useState(false)
  const [unpairTargetId, setUnpairTargetId] = useState<string | null>(null)

  const { setting } = useSetting()
  const globalAutoSyncOff = setting?.sync.autoSync === false
  const globalFileSyncOff = setting?.fileSync?.fileSyncEnabled === false
  const lanOnlyActive = setting?.network?.allowRelayFallback === false

  useEffect(() => {
    const handler = (event: { topic: string; eventType: string; payload: unknown }) => {
      if (event.topic !== 'peers') return
      if (event.eventType === 'peers.changed') {
        dispatch(fetchSpaceMembers())
      }
    }
    const unsub = daemonWs.subscribe(['peers'], handler)
    return unsub
  }, [dispatch])

  const handleSelectPeer = (peerId: string) => {
    setSelectedPeerId(peerId)
    setPeerDialogOpen(true)
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
      setPeerDialogOpen(false)
      setUnpairTargetId(null)
    } catch (error) {
      log.error({ err: error }, 'failed to unpair device')
    }
  }

  const selectedPeer = peers.find(d => d.peerId === selectedPeerId)
  const unpairTargetDevice = peers.find(d => d.peerId === unpairTargetId)

  // ── Mobile 状态 ──────────────────────────────────────────────
  const {
    devices: mobileDevices,
    devicesError: mobileDevicesError,
    settings: mobileSettings,
    addDialogOpen,
    settingsSheetOpen,
    enableConfirmOpen,
    credentialPayload,
    revokeTarget,
    revokeBusy,
    detailDevice: mobileDetailDevice,
    actions: mobileActions,
  } = useMobileDevices()

  // ── 渲染 ──────────────────────────────────────────────────────
  return (
    <>
      <Tabs value={tab} onValueChange={value => setTab(value as TabKey)} className="w-full">
        <div className="flex items-center justify-between gap-3 border-b border-border/50">
          <TabsList variant="line" className="h-11 gap-8 bg-transparent p-0">
            <TabsTrigger value="p2p" className="gap-2 px-0 text-sm font-medium">
              <ShieldCheck className="h-4 w-4" />
              {t('devices.pairedDevices.title')}
              <CountChip count={peers.length} active={tab === 'p2p'} />
            </TabsTrigger>
            <TabsTrigger value="mobile" className="gap-2 px-0 text-sm font-medium">
              <Smartphone className="h-4 w-4" />
              {t('devices.mobileSync.title')}
              <CountChip count={mobileDevices.length} active={tab === 'mobile'} />
            </TabsTrigger>
          </TabsList>

          <div className="flex items-center gap-1.5 pb-1">
            {/* key 上带 tab 名是为了让 tab 切换时 React 卸载旧按钮、挂载新按钮 —— */}
            {/* 否则两个分支被 reconciliation 复用同一 DOM,Button 的 transition-all */}
            {/* 会把 bg-primary → 透明的颜色变化做一次过渡,用户看到 primary 闪一下。 */}
            {tab === 'p2p' ? (
              <Button
                key="p2p-add"
                variant="default"
                size="sm"
                onClick={() => setAddP2PDialogOpen(true)}
              >
                <Plus className="h-3.5 w-3.5" />
                {t('devices.list.actions.addDevice')}
              </Button>
            ) : (
              <>
                <Button
                  key="mobile-configure"
                  variant="ghost"
                  size="sm"
                  onClick={mobileActions.openSettings}
                >
                  <Settings2 className="h-3.5 w-3.5" />
                  {t('devices.mobileSync.configure')}
                </Button>
                <Button
                  key="mobile-add"
                  variant="default"
                  size="sm"
                  onClick={mobileActions.handleAddClick}
                  disabled={mobileSettings?.lanListenerError != null}
                  title={
                    mobileSettings?.lanListenerError
                      ? t('devices.mobileSync.statusBar.bindFailed', {
                          reason: mobileSettings.lanListenerError,
                        })
                      : undefined
                  }
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t('devices.mobileSync.list.addButton')}
                </Button>
              </>
            )}
          </div>
        </div>

        <TabsContent value="p2p" className="mt-6">
          {spaceMembersError ? (
            <Alert variant="destructive">
              <AlertDescription className="flex items-center gap-3">
                <span className="flex-1">{spaceMembersError}</span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => {
                    dispatch(clearSpaceMembersError())
                    dispatch(fetchSpaceMembers())
                  }}
                >
                  <RefreshCw className="h-4 w-4" />
                </Button>
              </AlertDescription>
            </Alert>
          ) : (
            <PeerGrid
              peers={peers}
              lanOnlyActive={lanOnlyActive}
              onSelect={handleSelectPeer}
              onAdd={() => setAddP2PDialogOpen(true)}
            />
          )}
        </TabsContent>

        <TabsContent value="mobile" className="mt-6">
          {mobileDevicesError ? (
            <Alert variant="destructive">
              <AlertDescription className="flex items-center gap-3">
                <span className="flex-1">{mobileDevicesError}</span>
                <Button variant="ghost" size="icon-sm" onClick={mobileActions.reload}>
                  <RefreshCw className="h-4 w-4" />
                </Button>
              </AlertDescription>
            </Alert>
          ) : (
            <MobileGrid
              mobiles={mobileDevices}
              onSelect={mobileActions.openDetail}
              onAdd={mobileActions.handleAddClick}
            />
          )}
        </TabsContent>
      </Tabs>

      {/* ── P2P dialogs ────────────────────────────────────────── */}
      <AddDeviceDialog open={addP2PDialogOpen} onOpenChange={setAddP2PDialogOpen} />
      <DeviceSettingsDialog
        open={peerDialogOpen}
        onOpenChange={setPeerDialogOpen}
        deviceId={selectedPeerId || ''}
        device={selectedPeer}
        globalAutoSyncOff={globalAutoSyncOff}
        globalFileSyncOff={globalFileSyncOff}
        lanOnlyActive={lanOnlyActive}
        onUnpair={handleUnpairRequest}
      />
      <UnpairAlertDialog
        open={unpairDialogOpen}
        onOpenChange={setUnpairDialogOpen}
        deviceName={unpairTargetDevice?.deviceName || t('devices.list.labels.unknownDevice')}
        onConfirm={handleUnpairConfirm}
      />

      {/* ── Mobile dialogs / modals ────────────────────────────── */}
      <MobileSyncSettingsDialog
        open={settingsSheetOpen}
        onOpenChange={mobileActions.setSettingsSheetOpen}
        onSettingsChange={mobileActions.setSettings}
      />
      <EnableMobileSyncDialog
        open={enableConfirmOpen}
        onOpenChange={mobileActions.setEnableConfirmOpen}
        onSuccess={mobileActions.handleEnableSuccess}
      />
      <AddMobileSyncDeviceDialog
        open={addDialogOpen}
        onOpenChange={mobileActions.setAddDialogOpen}
        onSuccess={mobileActions.handleAddSuccess}
      />
      <MobileSyncDeviceDialog
        open={mobileDetailDevice !== null}
        onOpenChange={open => {
          if (!open) mobileActions.closeDetail()
        }}
        device={mobileDetailDevice}
        settings={mobileSettings}
        onRevoke={mobileActions.requestRevoke}
        onRotated={mobileActions.reload}
      />
      <MobileSyncCredentialModal
        payload={credentialPayload}
        onComplete={mobileActions.completeCredential}
      />

      <AlertDialog
        open={!!revokeTarget}
        onOpenChange={open => {
          if (!open && !revokeBusy) mobileActions.clearRevokeTarget()
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t('devices.mobileSync.revoke.confirmTitle', {
                label: revokeTarget?.label ?? '',
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.mobileSync.revoke.confirmDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={revokeBusy}>
              {t('devices.mobileSync.revoke.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={mobileActions.handleRevokeConfirm}
              disabled={revokeBusy}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {t('devices.mobileSync.revoke.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}

const CountChip: React.FC<{ count: number; active: boolean }> = ({ count, active }) => (
  <span
    className={cn(
      'inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-full px-1.5 text-[10px] font-semibold tabular-nums transition-colors',
      active ? 'bg-foreground text-background' : 'bg-muted text-muted-foreground'
    )}
  >
    {count}
  </span>
)

// ────────────────────────────────────────────────────────────────
// Peer 网格（P2P）
// ────────────────────────────────────────────────────────────────

interface PeerGridProps {
  peers: SpaceMember[]
  lanOnlyActive: boolean
  onSelect: (peerId: string) => void
  onAdd: () => void
}

const PeerGrid: React.FC<PeerGridProps> = ({ peers, lanOnlyActive, onSelect, onAdd }) => {
  const { t } = useTranslation()
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
      {peers.map(peer => (
        <PeerCard
          key={peer.peerId}
          peer={peer}
          lanOnlyActive={lanOnlyActive}
          onSelect={() => onSelect(peer.peerId)}
        />
      ))}
      <AddCard
        label={t('devices.addDevice.title')}
        hint={t('devices.addDevice.cardHint', {
          defaultValue: '生成邀请码 · 跨设备配对',
        })}
        onClick={onAdd}
      />
    </div>
  )
}

interface PeerCardProps {
  peer: SpaceMember
  lanOnlyActive: boolean
  onSelect: () => void
}

type ChannelTone = 'emerald' | 'amber' | 'muted'

const CHANNEL_ICON: Record<
  ReturnType<typeof deriveBadgeKind>,
  { icon: typeof Wifi; tone: ChannelTone }
> = {
  lan: { icon: Cable, tone: 'emerald' },
  relay: { icon: Wifi, tone: 'amber' },
  offline: { icon: WifiOff, tone: 'muted' },
  unknown: { icon: WifiOff, tone: 'muted' },
  outOfLan: { icon: WifiOff, tone: 'amber' },
}

const PeerCard: React.FC<PeerCardProps> = ({ peer, lanOnlyActive, onSelect }) => {
  const { t } = useTranslation()
  const Icon = getDeviceIcon(peer.deviceName)
  const kind = deriveBadgeKind(peer.channel ?? 'unknown', lanOnlyActive)
  const channel = CHANNEL_ICON[kind]

  return (
    <button
      type="button"
      onClick={onSelect}
      className="group relative flex w-full flex-col gap-4 overflow-hidden rounded-2xl border border-border/60 bg-card p-5 text-left transition-all hover:border-border hover:shadow-sm focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none"
    >
      <div className="flex items-start justify-between">
        <div
          className={cn(
            'relative flex h-12 w-12 items-center justify-center rounded-xl',
            peer.connected ? 'bg-success/10 text-success' : 'bg-muted text-muted-foreground'
          )}
        >
          {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon reference keyed on deviceName, not a freshly-created component */}
          <Icon className="h-6 w-6" />
          {peer.connected && (
            <span className="absolute -bottom-0.5 -right-0.5 h-3 w-3 rounded-full bg-success ring-2 ring-card" />
          )}
        </div>
        <ChevronRight className="h-4 w-4 text-muted-foreground/30 transition-all group-hover:translate-x-0.5 group-hover:text-muted-foreground" />
      </div>

      <div className="min-w-0">
        <h4 className="truncate text-base font-semibold leading-tight text-foreground">
          {peer.deviceName || t('devices.list.labels.unknownDevice')}
        </h4>
        <p className="mt-1 truncate text-xs text-muted-foreground">
          <span className={peer.connected ? 'text-success' : 'text-muted-foreground'}>
            {peer.connected
              ? `● ${t('devices.list.status.online')}`
              : `○ ${t('devices.list.status.offline')}`}
          </span>
          {peer.connectionAddress && (
            <>
              <span className="mx-1.5 text-muted-foreground/40">·</span>
              <span className="font-mono">{peer.connectionAddress}</span>
            </>
          )}
        </p>
      </div>

      <div className="pt-1">
        <ChannelChip
          icon={channel.icon}
          label={t(`devices.list.channel.${kind}`)}
          tone={channel.tone}
        />
      </div>
    </button>
  )
}

interface ChannelChipProps {
  icon: typeof Wifi
  label: string
  tone: ChannelTone
}

const CHANNEL_TONE: Record<ChannelTone, string> = {
  emerald: 'bg-success/10 text-success ring-success/20',
  amber: 'bg-warning/10 text-warning ring-warning/20',
  muted: 'bg-muted text-muted-foreground ring-border',
}

const ChannelChip: React.FC<ChannelChipProps> = ({ icon: Icon, label, tone }) => (
  <span
    className={cn(
      'inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] font-medium ring-1',
      CHANNEL_TONE[tone]
    )}
  >
    <Icon className="h-3 w-3" />
    {label}
  </span>
)

// ────────────────────────────────────────────────────────────────
// Mobile 网格
// ────────────────────────────────────────────────────────────────

interface MobileGridProps {
  mobiles: MobileDeviceView[]
  onSelect: (device: MobileDeviceView) => void
  onAdd: () => void
}

const MobileGrid: React.FC<MobileGridProps> = ({ mobiles, onSelect, onAdd }) => {
  const { t } = useTranslation()
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
      {mobiles.map(mobile => (
        <MobileCard key={mobile.deviceId} mobile={mobile} onSelect={() => onSelect(mobile)} />
      ))}
      <AddCard
        label={t('devices.mobileSync.add.title')}
        hint={t('devices.mobileSync.add.cardHint', {
          defaultValue: '生成凭据 · 适用 iOS / Android',
        })}
        onClick={onAdd}
      />
    </div>
  )
}

const MobileCard: React.FC<{
  mobile: MobileDeviceView
  onSelect: () => void
}> = ({ mobile, onSelect }) => {
  const { t } = useTranslation()
  const lastSeen = formatLastSeen(mobile.lastSeenAtMs ?? null, t)

  return (
    <button
      type="button"
      onClick={onSelect}
      className="group relative flex w-full flex-col gap-4 overflow-hidden rounded-2xl border border-border/60 bg-card p-5 text-left transition-all hover:border-border hover:shadow-sm focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none"
    >
      <div className="flex items-start justify-between">
        <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-info/10 text-info">
          <Smartphone className="h-6 w-6" />
        </div>
        <ChevronRight className="h-4 w-4 text-muted-foreground/30 transition-all group-hover:translate-x-0.5 group-hover:text-muted-foreground" />
      </div>

      <div className="min-w-0">
        <h4 className="truncate text-base font-semibold leading-tight text-foreground">
          {mobile.label}
        </h4>
        <p className="mt-1 truncate font-mono text-xs text-muted-foreground">{mobile.username}</p>
      </div>

      <div className="pt-1">
        <span className="text-[11px] text-muted-foreground">{lastSeen}</span>
      </div>
    </button>
  )
}

// ────────────────────────────────────────────────────────────────
// Add card (虚线占位)
// ────────────────────────────────────────────────────────────────

const AddCard: React.FC<{ label: string; hint: string; onClick: () => void }> = ({
  label,
  hint,
  onClick,
}) => (
  <button
    type="button"
    onClick={onClick}
    className="group flex flex-col items-center justify-center gap-3 rounded-2xl border-2 border-dashed border-border/60 bg-transparent p-5 text-center transition-all hover:border-primary/40 hover:bg-primary/5"
  >
    <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-muted text-muted-foreground transition-colors group-hover:bg-primary/10 group-hover:text-primary">
      <Plus className="h-6 w-6" />
    </div>
    <div>
      <p className="text-sm font-medium text-foreground">{label}</p>
      <p className="mt-1 text-[11px] text-muted-foreground">{hint}</p>
    </div>
  </button>
)

// ────────────────────────────────────────────────────────────────
// Mobile devices hook —— 集中存放原 MobileSyncDevicesPanel 的状态机
// ────────────────────────────────────────────────────────────────

interface UseMobileDevicesReturn {
  devices: MobileDeviceView[]
  devicesError: string | null
  settings: MobileSyncSettingsView | null
  addDialogOpen: boolean
  settingsSheetOpen: boolean
  enableConfirmOpen: boolean
  credentialPayload: RegisterMobileDeviceResult | null
  revokeTarget: MobileDeviceView | null
  revokeBusy: boolean
  /** 当前在 MobileSyncDeviceDialog 里查看的设备;null = dialog 关闭。 */
  detailDevice: MobileDeviceView | null
  actions: {
    reload: () => void
    handleAddClick: () => void
    handleEnableSuccess: () => void
    handleAddSuccess: (result: RegisterMobileDeviceResult) => void
    handleRevokeConfirm: () => Promise<void>
    requestRevoke: (device: MobileDeviceView) => void
    completeCredential: () => void
    clearRevokeTarget: () => void
    setAddDialogOpen: (open: boolean) => void
    setSettingsSheetOpen: (open: boolean) => void
    setEnableConfirmOpen: (open: boolean) => void
    setSettings: (settings: MobileSyncSettingsView | null) => void
    openSettings: () => void
    openDetail: (device: MobileDeviceView) => void
    closeDetail: () => void
  }
}

const useMobileDevices = (): UseMobileDevicesReturn => {
  const { t } = useTranslation()

  const [settings, setSettings] = useState<MobileSyncSettingsView | null>(null)
  const [devices, setDevices] = useState<MobileDeviceView[]>([])
  const [devicesError, setDevicesError] = useState<string | null>(null)

  const [settingsSheetOpen, setSettingsSheetOpen] = useState(false)
  const [addDialogOpen, setAddDialogOpen] = useState(false)
  const [enableConfirmOpen, setEnableConfirmOpen] = useState(false)
  const [credentialPayload, setCredentialPayload] = useState<RegisterMobileDeviceResult | null>(
    null
  )

  const [revokeTarget, setRevokeTarget] = useState<MobileDeviceView | null>(null)
  const [revokeBusy, setRevokeBusy] = useState(false)

  // detail dialog 不存 deviceId 而存整个 view; 列表 reload 后通过 deviceId
  // 重新 reconcile, 保证 dialog 内的字段始终是最新的服务端快照(关键是
  // lastSeen / reportedOs 在用户开 dialog 期间可能被 ws 事件更新)。
  // 改密 / 改密结果都在 dialog 内部组件状态里, 不再上提到 hook。
  const [detailDeviceId, setDetailDeviceId] = useState<string | null>(null)

  const translate = useCallback((err: unknown): string => translateMobileSyncError(t, err), [t])

  const reload = useCallback(async () => {
    try {
      const list = await listMobileDevices()
      setDevices(list)
      setDevicesError(null)
    } catch (err) {
      log.error({ err }, 'failed to list mobile devices')
      setDevicesError(translate(err))
    }
  }, [translate])

  // 首屏拉一次 settings：Add 按钮的 disable / 引导对话框分支都依赖
  // settings.enabled / lanListenerError，settings 只在 Settings 对话框
  // 打开时才会被回灌，初次进页面前必须主动取一次，否则 Add 永远走
  // EnableConfirm 流程、bind 失败的硬阻断也失效。
  useEffect(() => {
    void reload()
    getMobileSyncSettings()
      .then(setSettings)
      .catch(err => {
        log.warn({ err }, 'failed to preload mobile sync settings')
      })
  }, [reload])

  const handleAddClick = useCallback(() => {
    // bind 失败仍是硬阻断 —— 没起监听就没法接 iPhone, 任何 add 都没意义。
    if (settings?.lanListenerError) {
      toast.error(
        t('devices.mobileSync.statusBar.bindFailed', { reason: settings.lanListenerError })
      )
      return
    }
    // 首次入口：未开启 enabled / lan_listen → 弹引导对话框,确认后再 Add。
    if (!settings?.enabled || !settings?.lanListenEnabled) {
      setEnableConfirmOpen(true)
      return
    }
    setAddDialogOpen(true)
  }, [settings, t])

  const handleEnableSuccess = useCallback(() => {
    setAddDialogOpen(true)
  }, [])

  const handleAddSuccess = useCallback(
    (result: RegisterMobileDeviceResult) => {
      setCredentialPayload(result)
      void reload()
    },
    [reload]
  )

  // "撤销刚注册的设备"已下沉到设备卡片的 revoke 按钮(handleRevokeConfirm)。
  // 凭据 modal 现在只承担凭据展示 + 配对引导, 关闭路径统一收敛到
  // completeCredential — 用户后悔时去设备卡片删, 与"在 modal 里点 X"分流。
  const completeCredential = useCallback(() => {
    setCredentialPayload(null)
  }, [])

  const handleRevokeConfirm = useCallback(async () => {
    if (!revokeTarget) return
    setRevokeBusy(true)
    try {
      await revokeMobileDevice(revokeTarget.deviceId)
      toast.success(t('devices.mobileSync.revoke.confirmTitle', { label: revokeTarget.label }))
      setRevokeTarget(null)
      await reload()
    } catch (err) {
      log.error({ err, deviceId: revokeTarget.deviceId }, 'failed to revoke device')
      toast.error(translate(err))
    } finally {
      setRevokeBusy(false)
    }
  }, [reload, revokeTarget, t, translate])

  const detailDevice = useMemo(
    () => (detailDeviceId ? (devices.find(d => d.deviceId === detailDeviceId) ?? null) : null),
    [detailDeviceId, devices]
  )

  return {
    devices,
    devicesError,
    settings,
    addDialogOpen,
    settingsSheetOpen,
    enableConfirmOpen,
    credentialPayload,
    revokeTarget,
    revokeBusy,
    detailDevice,
    actions: {
      reload: () => void reload(),
      handleAddClick,
      handleEnableSuccess,
      handleAddSuccess,
      handleRevokeConfirm,
      requestRevoke: setRevokeTarget,
      completeCredential,
      clearRevokeTarget: () => setRevokeTarget(null),
      setAddDialogOpen,
      setSettingsSheetOpen,
      setEnableConfirmOpen,
      setSettings,
      openSettings: () => setSettingsSheetOpen(true),
      openDetail: (device: MobileDeviceView) => setDetailDeviceId(device.deviceId),
      closeDetail: () => setDetailDeviceId(null),
    },
  }
}

// ────────────────────────────────────────────────────────────────
// 辅助函数
// ────────────────────────────────────────────────────────────────

function translateMobileSyncError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileSync.errors.facadeUnavailable')
      case 'LAN_LISTENER_DISABLED':
        return t('devices.mobileSync.errors.lanListenerDisabled')
      case 'DEVICE_NOT_FOUND':
        return t('devices.mobileSync.errors.deviceNotFound')
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileSync.errors.persistenceFailed', { message: e.message })
      case 'SETTINGS_LOAD_FAILED':
        return t('devices.mobileSync.errors.settingsLoadFailed', { message: e.message })
      default: {
        const message = (e as { message?: string }).message ?? e.code
        return t('devices.mobileSync.errors.unknown', { message })
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileSync.errors.unknown', { message })
}

function formatLastSeen(
  lastSeenAtMs: number | null,
  t: ReturnType<typeof useTranslation>['t']
): string {
  if (lastSeenAtMs == null)
    return t('devices.mobileSync.list.lastSeen.never', { defaultValue: '从未活动' })
  const diffMs = Date.now() - lastSeenAtMs
  const diffMins = Math.round(diffMs / 60000)
  let rel: string
  if (diffMins < 1) rel = t('devices.mobileSync.list.lastSeen.justNow', { defaultValue: '刚刚' })
  else if (diffMins < 60) rel = `${diffMins}m`
  else if (diffMins < 1440) rel = `${Math.floor(diffMins / 60)}h`
  else rel = `${Math.floor(diffMins / 1440)}d`
  return t('devices.mobileSync.list.lastSeen.label', {
    defaultValue: '最近活动 · {{rel}}',
    rel,
  })
}
