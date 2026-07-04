/**
 * DevicesPage: master-detail layout ("Ledger" treatment).
 *
 *   ┌─ list column ──────┬─ detail pane ──────────────────────────┐
 *   │ 设备        2/3 在线 │  Windows 工作站            [取消配对]  │
 *   │ ── 本机 ──────────  │  ● 在线 · 局域网直连                    │
 *   │ ● MacBook Pro      │  ─────────────────────────────────────  │
 *   │ ── 已配对设备 ────  │  PEER ID   通道   地址                  │
 *   │ ● Windows 工作站 ◀ │  ─────────────────────────────────────  │
 *   │ ● Arch VM          │  同步设置（开关 + 内容类型）             │
 *   │ ── 移动同步 ──  ⚙  │                                         │
 *   │ ● iPhone 15 Pro    │                                         │
 *   │ ＋ 添加设备         │                                         │
 *   └────────────────────┴─────────────────────────────────────────┘
 *
 * Selecting a device renders its detail inline (LocalDevicePanel /
 * PeerDetailPanel / MobileDevicePanel) instead of stacking dialogs.
 * Flow dialogs (add / enable / credential echo / unpair / revoke /
 * mobile-sync settings) remain modal. `refreshPresence` fires only on
 * mount and on visibility regain; steady-state updates ride the
 * daemon-pushed `peers.changed` ws events.
 */

import { Plus, Settings2 } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
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
import EnableMobileSyncDialog from '@/components/device/EnableMobileSyncDialog'
import LocalDevicePanel from '@/components/device/LocalDevicePanel'
import MobileDevicePanel from '@/components/device/MobileDevicePanel'
import MobileSyncSettingsDialog from '@/components/device/MobileSyncSettingsDialog'
import PeerDetailPanel from '@/components/device/PeerDetailPanel'
import StatusDot, { type StatusDotTone } from '@/components/device/StatusDot'
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Skeleton } from '@/components/ui/skeleton'
import { toast } from '@/components/ui/toast'
import type { PeerSnapshotPayloadItem, PeersChangedPayload } from '@/hooks/useDaemonEvents'
import { useNow } from '@/hooks/useRelativeTime'
import { useSetting } from '@/hooks/useSetting'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  clearLocalDeviceError,
  clearSpaceMembersError,
  fetchLocalDeviceInfo,
  fetchSpaceMembers,
  setSpaceMembers,
} from '@/store/slices/devicesSlice'

const log = createLogger('devices-page')

type Selection = { kind: 'local' } | { kind: 'peer'; id: string } | { kind: 'mobile'; id: string }

/** A mobile device counts as "recently active" within this window. */
const MOBILE_ACTIVE_WINDOW_MS = 10 * 60 * 1000

const DevicesPage: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const now = useNow()

  const {
    localDevice,
    localDeviceLoading,
    localDeviceError,
    spaceMembers: rawSpaceMembers,
    spaceMembersError,
  } = useAppSelector(state => state.devices)

  const peers = localDevice
    ? rawSpaceMembers.filter(d => d.peerId !== localDevice.peerId)
    : rawSpaceMembers
  const onlineCount = peers.filter(p => p.connected).length

  const { setting } = useSetting()
  const syncActive = setting?.sync.autoSync !== false
  const globalAutoSyncOff = setting?.sync.autoSync === false
  const globalFileSyncOff = setting?.fileSync?.fileSyncEnabled === false
  const lanOnlyActive = setting?.network?.allowRelayFallback === false

  useEffect(() => {
    dispatch(fetchLocalDeviceInfo())
    dispatch(fetchSpaceMembers())

    // Presence awareness is push-driven by the daemon's PeerKeepAliveWorker
    // (inbound presence Online → outbound dial → peers.changed ws). The
    // frontend only pulls presence twice: on first mount (warm the UI) and
    // when the tab becomes visible again (safety snapshot). No polling.
    const probe = () => {
      refreshPresence().catch(err => {
        // refresh_presence 5xxs while setup is incomplete / daemon not
        // ready; the push pipeline is unaffected, so warn is enough.
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

  useEffect(() => {
    const handler = (event: { topic: string; eventType: string; payload: unknown }) => {
      if (event.topic !== 'peers') return
      if (event.eventType === 'peers.changed') {
        // The event carries the full member snapshot (same source as
        // GET /paired-devices), so apply it directly instead of firing a
        // redundant HTTP refetch on every presence flip (issue #1129).
        const payload = event.payload as PeersChangedPayload
        dispatch(setSpaceMembers(payload.peers.map(peerSnapshotToMember)))
      }
    }
    const unsub = daemonWs.subscribe(['peers'], handler)
    return unsub
  }, [dispatch])

  // ── mobile devices state ──────────────────────────────────────
  const {
    devices: mobileDevices,
    devicesError: mobileDevicesError,
    settings: mobileSettings,
    addDialogOpen,
    settingsSheetOpen,
    enableConfirmOpen,
    revokeTarget,
    revokeBusy,
    actions: mobileActions,
  } = useMobileDevices()

  // ── selection ────────────────────────────────────────────────
  const [selection, setSelection] = useState<Selection>({ kind: 'local' })
  // One-time credentials from a just-completed add. Handed to the selected
  // MobileDevicePanel so it shows the pairing QR + credentials + install helper
  // inline (the old post-registration modal, retired). Cleared as soon as the
  // user navigates to any other device so the one-time secret can't linger.
  const [pendingCredential, setPendingCredential] = useState<RegisterMobileDeviceResult | null>(
    null
  )
  useEffect(() => {
    if (
      pendingCredential &&
      !(selection.kind === 'mobile' && selection.id === pendingCredential.deviceId)
    ) {
      setPendingCredential(null)
    }
  }, [selection, pendingCredential])
  const selectedPeer =
    selection.kind === 'peer' ? peers.find(p => p.peerId === selection.id) : undefined
  const selectedMobile =
    selection.kind === 'mobile' ? mobileDevices.find(d => d.deviceId === selection.id) : undefined
  // A vanished selection (unpaired / revoked device) falls back to local.
  const effectiveSelection: Selection =
    (selection.kind === 'peer' && !selectedPeer) || (selection.kind === 'mobile' && !selectedMobile)
      ? { kind: 'local' }
      : selection

  // ── p2p dialogs ──────────────────────────────────────────────
  const [addP2PDialogOpen, setAddP2PDialogOpen] = useState(false)
  const [unpairDialogOpen, setUnpairDialogOpen] = useState(false)
  const [unpairTargetId, setUnpairTargetId] = useState<string | null>(null)

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
      setUnpairTargetId(null)
    } catch (error) {
      log.error({ err: error }, 'failed to unpair device')
    }
  }

  const unpairTargetDevice = peers.find(d => d.peerId === unpairTargetId)

  return (
    <div className="flex h-full">
      {/* ── list column ───────────────────────────────────────── */}
      <aside className="flex w-60 shrink-0 flex-col border-r border-border/50">
        <div className="px-4 pb-2 pt-5">
          <h2 className="text-base font-semibold tracking-tight text-foreground">
            {t('devices.panel.listTitle')}
          </h2>
          <p className="mt-0.5 text-[11px] text-muted-foreground">
            {t('devices.panel.counts', {
              online: onlineCount,
              total: peers.length,
              mobile: mobileDevices.length,
            })}
          </p>
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <div className="flex flex-col px-2 pb-3">
            {(spaceMembersError || mobileDevicesError) && (
              <Alert variant="destructive" className="mx-1 my-2">
                <AlertDescription className="flex flex-col gap-2 text-xs">
                  <span>{spaceMembersError ?? mobileDevicesError}</span>
                  <Button
                    variant="outline"
                    size="sm"
                    className="self-start"
                    onClick={() => {
                      if (spaceMembersError) {
                        dispatch(clearSpaceMembersError())
                        dispatch(fetchSpaceMembers())
                      }
                      if (mobileDevicesError) {
                        mobileActions.reload()
                      }
                    }}
                  >
                    {t('devices.list.actions.retry')}
                  </Button>
                </AlertDescription>
              </Alert>
            )}

            <SectionLabel label={t('devices.thisDevice.title')} />
            {localDevice ? (
              <DeviceListItem
                name={localDevice.deviceName}
                tone={syncActive ? 'success' : 'off'}
                trailing={t('devices.panel.localBadge')}
                selected={effectiveSelection.kind === 'local'}
                onSelect={() => setSelection({ kind: 'local' })}
              />
            ) : (
              <div className="px-2.5 py-2">
                {localDeviceError ? (
                  <button
                    type="button"
                    className="text-left text-xs text-destructive underline underline-offset-2"
                    onClick={() => {
                      dispatch(clearLocalDeviceError())
                      dispatch(fetchLocalDeviceInfo())
                    }}
                  >
                    {t('devices.list.actions.retry')}
                  </button>
                ) : (
                  <Skeleton className="h-5 w-32" />
                )}
              </div>
            )}

            <SectionLabel label={t('devices.pairedDevices.title')} />
            {peers.map(peer => (
              <DeviceListItem
                key={peer.peerId}
                name={peer.deviceName || t('devices.list.labels.unknownDevice')}
                tone={peerDotTone(peer, lanOnlyActive)}
                dimmed={!peer.connected}
                selected={
                  effectiveSelection.kind === 'peer' && effectiveSelection.id === peer.peerId
                }
                onSelect={() => setSelection({ kind: 'peer', id: peer.peerId })}
              />
            ))}
            {peers.length === 0 && !spaceMembersError && (
              <p className="px-2.5 py-1.5 text-[11px] text-muted-foreground/70">
                {t('devices.pairedDevices.subtitleEmpty')}
              </p>
            )}

            <SectionLabel
              label={t('devices.mobileSync.title')}
              trailing={
                <button
                  type="button"
                  aria-label={t('devices.mobileSync.configure')}
                  title={t('devices.mobileSync.configure')}
                  className="rounded-md p-0.5 text-muted-foreground/70 transition-colors hover:text-foreground"
                  onClick={mobileActions.openSettings}
                >
                  <Settings2 className="size-3.5" />
                </button>
              }
            />
            {mobileDevices.map(mobile => {
              const tone = mobileDotTone(mobile, now)
              return (
                <DeviceListItem
                  key={mobile.deviceId}
                  name={mobile.label}
                  tone={tone}
                  dimmed={tone === 'off'}
                  selected={
                    effectiveSelection.kind === 'mobile' &&
                    effectiveSelection.id === mobile.deviceId
                  }
                  onSelect={() => setSelection({ kind: 'mobile', id: mobile.deviceId })}
                />
              )
            })}
            {mobileDevices.length === 0 && !mobileDevicesError && (
              <p className="px-2.5 py-1.5 text-[11px] text-muted-foreground/70">
                {t('devices.mobileSync.list.empty.title')}
              </p>
            )}
          </div>
        </ScrollArea>

        <div className="border-t border-border/50 p-2">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="w-full justify-start gap-2 text-muted-foreground hover:text-foreground"
              >
                <Plus className="size-3.5" />
                {t('devices.panel.addMenu.trigger')}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-52">
              <DropdownMenuItem onSelect={() => setAddP2PDialogOpen(true)}>
                {t('devices.panel.addMenu.p2p')}
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={mobileSettings?.lanListenerError != null}
                onSelect={mobileActions.handleAddClick}
              >
                {t('devices.panel.addMenu.mobile')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </aside>

      {/* ── detail pane ───────────────────────────────────────── */}
      <main className="min-w-0 flex-1">
        <ScrollArea className="h-full">
          {effectiveSelection.kind === 'local' &&
            (localDevice ? (
              <LocalDevicePanel localDevice={localDevice} memberCount={peers.length + 1} />
            ) : localDeviceError ? (
              <div className="mx-auto w-full max-w-2xl px-8 py-8">
                <Alert variant="destructive">
                  <AlertDescription className="flex items-center gap-3">
                    <span className="flex-1">{localDeviceError}</span>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        dispatch(clearLocalDeviceError())
                        dispatch(fetchLocalDeviceInfo())
                      }}
                    >
                      {t('devices.list.actions.retry')}
                    </Button>
                  </AlertDescription>
                </Alert>
              </div>
            ) : (
              localDeviceLoading && <LocalPanelSkeleton />
            ))}

          {effectiveSelection.kind === 'peer' && selectedPeer && (
            <PeerDetailPanel
              key={selectedPeer.peerId}
              deviceId={selectedPeer.peerId}
              device={selectedPeer}
              globalAutoSyncOff={globalAutoSyncOff}
              globalFileSyncOff={globalFileSyncOff}
              lanOnlyActive={lanOnlyActive}
              onUnpair={handleUnpairRequest}
            />
          )}

          {effectiveSelection.kind === 'mobile' && selectedMobile && (
            <MobileDevicePanel
              key={selectedMobile.deviceId}
              device={selectedMobile}
              settings={mobileSettings}
              initialCredential={
                pendingCredential?.deviceId === selectedMobile.deviceId ? pendingCredential : null
              }
              onRevoke={mobileActions.requestRevoke}
              onRotated={mobileActions.reload}
            />
          )}
        </ScrollArea>
      </main>

      {/* ── flow dialogs ──────────────────────────────────────── */}
      <AddDeviceDialog open={addP2PDialogOpen} onOpenChange={setAddP2PDialogOpen} />
      <UnpairAlertDialog
        open={unpairDialogOpen}
        onOpenChange={setUnpairDialogOpen}
        deviceName={unpairTargetDevice?.deviceName || t('devices.list.labels.unknownDevice')}
        onConfirm={handleUnpairConfirm}
      />
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
        onSuccess={result => {
          // Retire the credential modal: refresh the list, select the new
          // device, and hand its one-time credentials to the panel's fresh
          // state (pairing QR + credentials + install helper) inline.
          mobileActions.reload()
          setPendingCredential(result)
          setSelection({ kind: 'mobile', id: result.deviceId })
        }}
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
    </div>
  )
}

export default DevicesPage

// ────────────────────────────────────────────────────────────────
// List column pieces
// ────────────────────────────────────────────────────────────────

const SectionLabel: React.FC<{ label: string; trailing?: React.ReactNode }> = ({
  label,
  trailing,
}) => (
  <div className="flex items-center justify-between px-2.5 pb-1 pt-4">
    <span className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground/80">
      {label}
    </span>
    {trailing}
  </div>
)

interface DeviceListItemProps {
  name: string
  tone: StatusDotTone
  selected: boolean
  dimmed?: boolean
  trailing?: string
  onSelect: () => void
}

const DeviceListItem: React.FC<DeviceListItemProps> = ({
  name,
  tone,
  selected,
  dimmed,
  trailing,
  onSelect,
}) => (
  <button
    type="button"
    onClick={onSelect}
    className={cn(
      'flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm transition-colors',
      selected
        ? 'bg-muted font-semibold text-foreground'
        : 'font-medium text-foreground hover:bg-muted/60',
      dimmed && !selected && 'opacity-60'
    )}
  >
    <StatusDot tone={tone} />
    <span className="min-w-0 flex-1 truncate">{name}</span>
    {trailing && (
      <span className="shrink-0 text-[10px] font-normal text-muted-foreground">{trailing}</span>
    )}
  </button>
)

const LocalPanelSkeleton: React.FC = () => (
  <div className="mx-auto flex w-full max-w-2xl flex-col gap-6 px-8 py-8">
    <div className="flex items-center gap-4">
      <Skeleton className="size-12 rounded-xl" />
      <div className="flex flex-1 flex-col gap-2">
        <Skeleton className="h-5 w-44" />
        <Skeleton className="h-3 w-28" />
      </div>
    </div>
    <Skeleton className="h-16 w-full" />
  </div>
)

// ────────────────────────────────────────────────────────────────
// Presence helpers
// ────────────────────────────────────────────────────────────────

function peerDotTone(peer: SpaceMember, lanOnlyActive: boolean): StatusDotTone {
  if (!peer.connected) return 'off'
  const kind = deriveBadgeKind(peer.channel ?? 'unknown', lanOnlyActive)
  return kind === 'relay' ? 'warning' : 'success'
}

function mobileDotTone(mobile: MobileDeviceView, now: number): StatusDotTone {
  if (mobile.lastSeenAtMs == null) return 'off'
  return now - mobile.lastSeenAtMs <= MOBILE_ACTIVE_WINDOW_MS ? 'info' : 'off'
}

// ────────────────────────────────────────────────────────────────
// Mobile devices hook (state machine shared by list + panels + dialogs)
// ────────────────────────────────────────────────────────────────

interface UseMobileDevicesReturn {
  devices: MobileDeviceView[]
  devicesError: string | null
  settings: MobileSyncSettingsView | null
  addDialogOpen: boolean
  settingsSheetOpen: boolean
  enableConfirmOpen: boolean
  revokeTarget: MobileDeviceView | null
  revokeBusy: boolean
  actions: {
    reload: () => void
    handleAddClick: () => void
    handleEnableSuccess: () => void
    handleRevokeConfirm: () => Promise<void>
    requestRevoke: (device: MobileDeviceView) => void
    clearRevokeTarget: () => void
    setAddDialogOpen: (open: boolean) => void
    setSettingsSheetOpen: (open: boolean) => void
    setEnableConfirmOpen: (open: boolean) => void
    setSettings: (settings: MobileSyncSettingsView | null) => void
    openSettings: () => void
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

  const [revokeTarget, setRevokeTarget] = useState<MobileDeviceView | null>(null)
  const [revokeBusy, setRevokeBusy] = useState(false)

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

  // Preload settings on first render: the add flow's gating (enable
  // confirm / bind-error hard block) reads settings.enabled and
  // settings.lanListenerError, which are otherwise only refreshed when
  // the settings dialog opens.
  useEffect(() => {
    void reload()
    getMobileSyncSettings()
      .then(setSettings)
      .catch(err => {
        log.warn({ err }, 'failed to preload mobile sync settings')
      })
  }, [reload])

  const handleAddClick = useCallback(() => {
    // A failed LAN bind is a hard block: without a listener no mobile
    // device can connect, so adding one is pointless.
    if (settings?.lanListenerError) {
      toast.error(
        t('devices.mobileSync.statusBar.bindFailed', { reason: settings.lanListenerError })
      )
      return
    }
    // First-run path: mobile sync or the LAN listener is off, walk the
    // user through the enable confirm before the add dialog.
    if (!settings?.enabled || !settings?.lanListenEnabled) {
      setEnableConfirmOpen(true)
      return
    }
    setAddDialogOpen(true)
  }, [settings, t])

  const handleEnableSuccess = useCallback(() => {
    setAddDialogOpen(true)
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

  return {
    devices,
    devicesError,
    settings,
    addDialogOpen,
    settingsSheetOpen,
    enableConfirmOpen,
    revokeTarget,
    revokeBusy,
    actions: {
      reload: () => void reload(),
      handleAddClick,
      handleEnableSuccess,
      handleRevokeConfirm,
      requestRevoke: setRevokeTarget,
      clearRevokeTarget: () => setRevokeTarget(null),
      setAddDialogOpen,
      setSettingsSheetOpen,
      setEnableConfirmOpen,
      setSettings,
      openSettings: () => setSettingsSheetOpen(true),
    },
  }
}

// ────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────

/**
 * Project a `peers.changed` snapshot entry into the `SpaceMember` view model.
 * Mirrors how `GET /paired-devices` projects the same daemon source: P2P peers
 * never carry `lastSeenAtMs` (always null) and an empty `deviceName` collapses
 * to "". Keeping the two paths identical means the WS fast-path produces byte-
 * for-byte the same member list the HTTP refetch would have.
 */
function peerSnapshotToMember(p: PeerSnapshotPayloadItem): SpaceMember {
  return {
    peerId: p.peerId,
    deviceName: p.deviceName ?? '',
    pairingState: p.pairingState ?? 'Trusted',
    lastSeenAtMs: null,
    connected: p.connected,
    channel: p.channel ?? 'unknown',
    connectionAddress: p.connectionAddress ?? null,
  }
}

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
