/**
 * DevicesPage: master-detail layout ("Ledger" treatment).
 *
 *   ┌─ list column ──────┬─ detail pane ──────────────────────────┐
 *   │ 设备       [＋ 添加] │  Windows 工作站            [取消配对]  │
 *   │ 2/3 在线            │  ● 在线 · 局域网直连                    │
 *   │ ── 本机 ──────────  │  ─────────────────────────────────────  │
 *   │ ● MacBook Pro      │  PEER ID   通道   地址                  │
 *   │ ── 已配对设备 ────  │  ─────────────────────────────────────  │
 *   │ ● Windows 工作站 ◀ │  同步设置（开关 + 内容类型）             │
 *   │ ● Arch VM          │                                         │
 *   │ ── 移动同步 ──  ⚙  │                                         │
 *   │ ● iPhone 15 Pro    │                                         │
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
import React, { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { useTranslation } from 'react-i18next'
import { refreshPresence } from '@/api/daemon'
import {
  continueMemberRemoval,
  getCurrentMemberRemoval,
  isLegacyBootstrapRequired,
  isMemberRemovalInProgress,
  secureRemoveLegacyMember,
  type MemberRemoval,
} from '@/api/daemon/member'
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
import { derivePeerStatusTone } from '@/components/device/connection-channel-utils'
import EnableMobileSyncDialog from '@/components/device/EnableMobileSyncDialog'
import LocalDevicePanel from '@/components/device/LocalDevicePanel'
import MemberRemovalStatus from '@/components/device/MemberRemovalStatus'
import MobileDevicePanel from '@/components/device/MobileDevicePanel'
import MobileSyncSettingsDialog from '@/components/device/MobileSyncSettingsDialog'
import PeerDetailPanel from '@/components/device/PeerDetailPanel'
import SpaceConnectionStatus from '@/components/device/SpaceConnectionStatus'
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
  fetchMembershipConvergence,
  fetchCurrentMemberRemoval,
  fetchSpaceProtection,
  fetchSpaceMembers,
  setSpaceMembers,
  setMemberRemoval,
} from '@/store/slices/devicesSlice'

const log = createLogger('devices-page')

type Selection =
  | { kind: 'local' }
  | { kind: 'peer'; id: string }
  | { kind: 'mobile'; id: string; pendingCredential?: RegisterMobileDeviceResult }

/** A mobile device counts as "recently active" within this window. */
const MOBILE_ACTIVE_WINDOW_MS = 10 * 60 * 1000
const CONVERGENCE_POLL_MS = 2_000
const CONVERGENCE_POLL_ATTEMPTS = 15

function subscribeDocumentVisibility(onStoreChange: () => void): () => void {
  document.addEventListener('visibilitychange', onStoreChange)
  return () => document.removeEventListener('visibilitychange', onStoreChange)
}

function getDocumentVisible(): boolean {
  return document.visibilityState === 'visible'
}

const DevicesPage: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const now = useNow()
  const documentVisible = useSyncExternalStore(
    subscribeDocumentVisibility,
    getDocumentVisible,
    () => true
  )

  const {
    localDevice,
    localDeviceLoading,
    localDeviceError,
    spaceMembers: rawSpaceMembers,
    spaceMembersError,
    spaceProtection,
    spaceProtectionError,
    membershipConvergence,
    membershipConvergenceError,
    memberRemoval,
  } = useAppSelector(state => state.devices)

  const peers = localDevice
    ? rawSpaceMembers.filter(d => d.peerId !== localDevice.peerId)
    : rawSpaceMembers
  const onlineCount = peers.filter(p => p.connected).length
  const legacyBootstrap = spaceProtection?.legacyBootstrap ?? null

  const { setting } = useSetting()
  const syncActive = setting?.sync.autoSync !== false
  const globalAutoSyncOff = setting?.sync.autoSync === false
  const globalFileSyncOff = setting?.fileSync?.fileSyncEnabled === false
  const lanOnlyActive = setting?.network?.allowRelayFallback === false

  useEffect(() => {
    dispatch(fetchLocalDeviceInfo())
    dispatch(fetchSpaceMembers())
    dispatch(fetchCurrentMemberRemoval())
  }, [dispatch])

  useEffect(() => {
    if (!documentVisible) return
    dispatch(fetchSpaceProtection())
    dispatch(fetchMembershipConvergence())

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
  }, [dispatch, documentVisible])

  useEffect(() => {
    if (membershipConvergence?.state !== 'converging' || !documentVisible) return

    let attempts = 0
    const timer = window.setInterval(() => {
      attempts += 1
      dispatch(fetchMembershipConvergence())
      if (attempts >= CONVERGENCE_POLL_ATTEMPTS) window.clearInterval(timer)
    }, CONVERGENCE_POLL_MS)

    return () => window.clearInterval(timer)
  }, [dispatch, documentVisible, membershipConvergence?.state])

  useEffect(() => {
    const handler = (event: { topic: string; eventType: string; payload: unknown }) => {
      if (event.topic === 'member-removal' && event.eventType === 'member-removal.changed') {
        dispatch(fetchCurrentMemberRemoval())
        dispatch(fetchSpaceMembers())
        return
      }
      if (event.topic !== 'peers') return
      if (event.eventType === 'peers.changed') {
        // The event carries the full member snapshot (same source as
        // GET /paired-devices), so apply it directly instead of firing a
        // redundant HTTP refetch on every presence flip (issue #1129).
        const payload = event.payload as PeersChangedPayload
        dispatch(setSpaceMembers(payload.peers.map(peerSnapshotToMember)))
        if (documentVisible) {
          dispatch(fetchMembershipConvergence())
        }
      }
    }
    const unsub = daemonWs.subscribe(['peers', 'member-removal'], handler)
    return unsub
  }, [dispatch, documentVisible])

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
  const selectedPeer =
    selection.kind === 'peer' ? peers.find(p => p.peerId === selection.id) : undefined
  const selectedMobile =
    selection.kind === 'mobile' ? mobileDevices.find(d => d.deviceId === selection.id) : undefined
  // An unresolvable selection falls back to the local panel for this render
  // only — it must not be written back into `selection`. The id may simply not
  // have loaded yet: the add flow selects the new device while its
  // `reload()` is still in flight, and resetting the state here would strand
  // the user on the local panel once the device does arrive. A device that
  // really vanished (unpaired / revoked) never comes back, so it keeps
  // falling back for every subsequent render anyway.
  const effectiveSelection: Selection =
    (selection.kind === 'peer' && !selectedPeer) || (selection.kind === 'mobile' && !selectedMobile)
      ? { kind: 'local' }
      : selection

  // ── p2p dialogs ──────────────────────────────────────────────
  const [addP2PDialogOpen, setAddP2PDialogOpen] = useState(false)
  const [unpairDialogOpen, setUnpairDialogOpen] = useState(false)
  const [unpairTargetId, setUnpairTargetId] = useState<string | null>(null)
  const [unpairBusy, setUnpairBusy] = useState(false)
  const [permanentLossRemoval, setPermanentLossRemoval] = useState<MemberRemoval | null>(null)
  const [permanentLossDeviceIds, setPermanentLossDeviceIds] = useState<string[]>([])
  const [permanentLossBusy, setPermanentLossBusy] = useState(false)
  const unpairBusyRef = useRef(false)

  const handleUnpairRequest = (peerId: string) => {
    if (unpairBusyRef.current) return
    setUnpairTargetId(peerId)
    setUnpairDialogOpen(true)
  }

  const handleUnpairDialogOpenChange = (open: boolean) => {
    if (!open && unpairBusyRef.current) return
    setUnpairDialogOpen(open)
    if (!open) setUnpairTargetId(null)
  }

  const handleUnpairConfirm = async () => {
    if (!unpairTargetId || unpairBusyRef.current) return
    unpairBusyRef.current = true
    setUnpairBusy(true)
    try {
      try {
        const removal = await unpairDevice(unpairTargetId)
        dispatch(setMemberRemoval(removal))
      } catch (error) {
        if (isMemberRemovalInProgress(error)) {
          const current = await getCurrentMemberRemoval()
          dispatch(setMemberRemoval(current))
          setUnpairDialogOpen(false)
          setUnpairTargetId(null)
          return
        }
        if (!isLegacyBootstrapRequired(error)) {
          log.error({ err: error }, 'failed to remove device')
          toast.error(t('devices.protection.errors.removeFailed'))
          return
        }
        try {
          await secureRemoveLegacyMember(unpairTargetId)
        } catch (bootstrapError) {
          log.error({ err: bootstrapError }, 'failed to start secure legacy removal')
          toast.error(t('devices.protection.errors.removeFailed'))
          return
        }
      }

      dispatch(fetchSpaceMembers())
      dispatch(fetchSpaceProtection())
      setSelection({ kind: 'local' })
      setUnpairDialogOpen(false)
      setUnpairTargetId(null)
    } finally {
      unpairBusyRef.current = false
      setUnpairBusy(false)
    }
  }

  const unpairTargetDevice = peers.find(d => d.peerId === unpairTargetId)
  const hideDuplicateUpgrade =
    membershipConvergence?.state === 'waiting_for_upgrade' &&
    legacyBootstrap !== null &&
    legacyBootstrap.outcome !== 'complete'
  const visibleConvergenceState =
    membershipConvergenceError || hideDuplicateUpgrade
      ? 'complete'
      : (membershipConvergence?.state ?? 'complete')
  const deviceNames = new Map(rawSpaceMembers.map(device => [device.peerId, device.deviceName]))
  const handlePermanentLoss = (removal: MemberRemoval, deviceIds: string[]) => {
    setPermanentLossRemoval(removal)
    setPermanentLossDeviceIds(deviceIds)
  }
  const confirmPermanentLoss = async () => {
    if (!permanentLossRemoval?.revocationId || permanentLossBusy) return
    setPermanentLossBusy(true)
    try {
      const next = await continueMemberRemoval(
        permanentLossRemoval.revocationId,
        permanentLossDeviceIds
      )
      dispatch(setMemberRemoval(next))
      dispatch(fetchSpaceMembers())
      setPermanentLossRemoval(null)
      setPermanentLossDeviceIds([])
    } catch (error) {
      log.error({ err: error }, 'failed to continue member removal')
      toast.error(t('devices.memberRemoval.errors.continueFailed'))
    } finally {
      setPermanentLossBusy(false)
    }
  }

  return (
    <div className="flex h-full">
      {/* ── list column ───────────────────────────────────────── */}
      <aside className="relative flex w-60 shrink-0 flex-col border-r border-border/50">
        {/* The add button shares a row with the title only, so the counts
            line below keeps the full column width instead of wrapping in a
            long locale (ru). */}
        <div className="px-4 pb-2 pt-5">
          <div className="flex items-center justify-between gap-2">
            <h2 className="min-w-0 truncate text-base font-semibold tracking-tight text-foreground">
              {t('devices.panel.listTitle')}
            </h2>
            {/* The button shows `triggerShort` but announces the full
                `trigger` wording. WCAG 2.5.3 (Label in Name) requires the
                accessible name to contain the visible text, so every locale
                must keep triggerShort a substring of trigger. */}
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    size="xs"
                    aria-label={t('devices.panel.addMenu.trigger')}
                  />
                }
              >
                <Plus />
                {t('devices.panel.addMenu.triggerShort')}
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-52">
                <DropdownMenuItem onClick={() => setAddP2PDialogOpen(true)}>
                  {t('devices.panel.addMenu.p2p')}
                </DropdownMenuItem>
                {/* Not disabled on `lanListenerError`: `handleAddClick` owns
                    that gate and explains the failure via toast. A disabled
                    item would block the only path to that explanation. */}
                <DropdownMenuItem onClick={mobileActions.handleAddClick}>
                  {t('devices.panel.addMenu.mobile')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
          <p className="mt-0.5 text-[11px] text-muted-foreground">
            {t('devices.panel.counts', {
              online: onlineCount,
              total: peers.length,
              mobile: mobileDevices.length,
            })}
          </p>
          <SpaceConnectionStatus state={visibleConvergenceState} />
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <div className="flex flex-col px-2 pb-3">
            {(spaceMembersError || mobileDevicesError || spaceProtectionError) && (
              <Alert variant="destructive" className="mx-1 my-2">
                <AlertDescription className="flex flex-col gap-2 text-xs">
                  <span>
                    {spaceMembersError ??
                      mobileDevicesError ??
                      (spaceProtectionError ? t(spaceProtectionError) : null)}
                  </span>
                  <Button
                    variant="outline"
                    size="sm"
                    className="self-start"
                    onClick={() => {
                      if (spaceMembersError) {
                        dispatch(clearSpaceMembersError())
                        dispatch(fetchSpaceMembers())
                      }
                      if (spaceProtectionError) {
                        dispatch(fetchSpaceProtection())
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

            {legacyBootstrap && legacyBootstrap.outcome !== 'complete' && (
              <Alert className="mx-1 my-2 border-warning/30 bg-warning/10 text-warning">
                <AlertDescription className="text-xs">
                  {t(`devices.protection.bootstrap.${legacyBootstrap.outcome}`, {
                    count: legacyBootstrap.pendingReadmission,
                  })}
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
                tone={peerDotTone(peer)}
                dimmed={!peer.connected}
                selected={
                  effectiveSelection.kind === 'peer' && effectiveSelection.id === peer.peerId
                }
                onSelect={() => setSelection({ kind: 'peer', id: peer.peerId })}
              />
            ))}
            {peers.length === 0 && !spaceMembersError && (
              <EmptyAddRow
                label={t('devices.panel.addMenu.p2p')}
                onClick={() => setAddP2PDialogOpen(true)}
              />
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
                  onSelect={() =>
                    setSelection(current => ({
                      kind: 'mobile',
                      id: mobile.deviceId,
                      pendingCredential:
                        current.kind === 'mobile' && current.id === mobile.deviceId
                          ? current.pendingCredential
                          : undefined,
                    }))
                  }
                />
              )
            })}
            {mobileDevices.length === 0 && !mobileDevicesError && (
              <EmptyAddRow
                label={t('devices.panel.addMenu.mobile')}
                onClick={mobileActions.handleAddClick}
              />
            )}
          </div>
        </ScrollArea>
        {memberRemoval && (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 z-20 p-3">
            <MemberRemovalStatus
              className="pointer-events-auto"
              removal={memberRemoval}
              deviceNames={deviceNames}
              onPermanentLoss={handlePermanentLoss}
            />
          </div>
        )}
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
                selection.kind === 'mobile' && selection.id === selectedMobile.deviceId
                  ? (selection.pendingCredential ?? null)
                  : null
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
        onOpenChange={handleUnpairDialogOpenChange}
        deviceName={unpairTargetDevice?.deviceName || t('devices.list.labels.unknownDevice')}
        busy={unpairBusy}
        onConfirm={handleUnpairConfirm}
      />
      <AlertDialog
        open={permanentLossRemoval !== null}
        onOpenChange={open => {
          if (!open && !permanentLossBusy) {
            setPermanentLossRemoval(null)
            setPermanentLossDeviceIds([])
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('devices.memberRemoval.permanentLoss.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.memberRemoval.permanentLoss.description')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={permanentLossBusy}>
              {t('clipboard.cancelLabel')}
            </AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={permanentLossBusy}
              onClick={confirmPermanentLoss}
            >
              {t('devices.memberRemoval.permanentLoss.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
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
          setSelection({ kind: 'mobile', id: result.deviceId, pendingCredential: result })
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

/**
 * Empty-state row that doubles as that section's add entry point: with no
 * devices to list, the space is better spent on a labelled target than on a
 * dead "nothing here" line. The header's `＋` stays the steady-state shortcut.
 *
 * Deliberately never disabled - the click handler owns any gating and can say
 * why it refused, which a greyed-out row cannot.
 */
const EmptyAddRow: React.FC<{ label: string; onClick: () => void }> = ({ label, onClick }) => (
  <button
    type="button"
    onClick={onClick}
    className="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-left text-[11px] text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground"
  >
    <Plus className="size-3" />
    {label}
  </button>
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

function peerDotTone(peer: SpaceMember): StatusDotTone {
  return derivePeerStatusTone(peer.channel ?? 'unknown', peer.connected)
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
