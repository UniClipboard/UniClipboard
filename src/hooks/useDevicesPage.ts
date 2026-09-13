import {
  useCallback,
  useEffect,
  useId,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'
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
import {
  buildDeviceTrustListView,
  getDeviceTrustStatus,
  type DeviceRowStatus,
} from '@/components/device/device-trust-view'
import { toast } from '@/components/ui/toast'
import type { PeerSnapshotPayloadItem, PeersChangedPayload } from '@/hooks/useDaemonEvents'
import { useDeviceTrust } from '@/hooks/useDeviceTrust'
import { useNow } from '@/hooks/useRelativeTime'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  fetchLocalDeviceInfo,
  fetchNetworkRecoveryStatus,
  fetchSpaceMembers,
  fetchSpaceProtection,
  refreshDeviceConnections,
  requestNetworkRecovery,
  setSpaceMembers,
} from '@/store/slices/devicesSlice'

const log = createLogger('devices-page')

type Selection =
  | { kind: 'local' }
  | { kind: 'peer'; id: string }
  | {
      kind: 'mobile'
      id: string
      pendingCredential?: RegisterMobileDeviceResult
    }

type UnpairState = { targetId: string | null; busy: boolean }
type UnpairAction =
  | { type: 'select'; id: string }
  | { type: 'close' }
  | { type: 'start' }
  | { type: 'settled' }

function unpairReducer(state: UnpairState, action: UnpairAction): UnpairState {
  switch (action.type) {
    case 'select':
      return { targetId: action.id, busy: false }
    case 'close':
      return { ...state, targetId: null }
    case 'start':
      return { ...state, busy: true }
    case 'settled':
      return { ...state, busy: false }
  }
}

function subscribeDocumentVisibility(onStoreChange: () => void): () => void {
  document.addEventListener('visibilitychange', onStoreChange)
  return () => document.removeEventListener('visibilitychange', onStoreChange)
}

function getDocumentVisible(): boolean {
  return document.visibilityState === 'visible'
}

export function useDevicesPage() {
  const selectionId = useId()
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
    spaceProtectionError,
    networkRecovery,
    networkRecoveryError,
    networkRecoveryRequestId,
    connectionRefresh,
    connectionRefreshTrigger,
  } = useAppSelector(
    ({ devices }) => ({
      localDevice: devices.localDevice,
      localDeviceLoading: devices.localDeviceLoading,
      localDeviceError: devices.localDeviceError,
      spaceMembers: devices.spaceMembers,
      spaceMembersError: devices.spaceMembersError,
      spaceProtectionError: devices.spaceProtectionError,
      networkRecovery: devices.networkRecovery,
      networkRecoveryError: devices.networkRecoveryError,
      networkRecoveryRequestId: devices.networkRecoveryRequestId,
      connectionRefresh: devices.connectionRefresh,
      connectionRefreshTrigger: devices.connectionRefreshTrigger,
    }),
    shallowEqual
  )
  const { snapshot: deviceTrust, refresh: refreshDeviceTrust } = useDeviceTrust()
  const manualRefreshInProgress =
    connectionRefresh.status === 'checking' && connectionRefreshTrigger === 'manual'

  const admittedPeers = localDevice
    ? rawSpaceMembers.filter(d => d.peerId !== localDevice.peerId)
    : rawSpaceMembers
  const trustListView = buildDeviceTrustListView(admittedPeers, deviceTrust)
  const peers = trustListView.peers
  const onlineCount = peers.filter(p => p.connected).length

  const localTrust = trustListView.localRelationship
  const localDeviceStatus: DeviceRowStatus | undefined =
    deviceTrust?.localMembership === 'removed'
      ? {
          kind: 'removed',
          label: t('devices.memberRemoval.deviceRemoved.title'),
          description: t('devices.memberRemoval.deviceRemoved.description'),
        }
      : localTrust?.groupRelationship === 'unverifiable'
        ? {
            kind: 'recovery_required',
            label: t('devices.memberRemoval.recoveryRequired.title'),
            description: t('devices.memberRemoval.recoveryRequired.description'),
          }
        : localTrust?.groupRelationship === 'pending_local_decision'
          ? {
              kind: 'removing',
              label: t('devices.memberRemoval.converging.title'),
              description: t('devices.memberRemoval.converging.description'),
            }
          : undefined

  useEffect(() => {
    dispatch(fetchLocalDeviceInfo())
    dispatch(fetchSpaceMembers())
  }, [dispatch])

  useEffect(() => {
    if (!documentVisible) return
    dispatch(fetchSpaceProtection())
    dispatch(fetchNetworkRecoveryStatus())

    // Share the same refresh with the manual action; live updates remain push-driven.
    void dispatch(refreshDeviceConnections())
  }, [dispatch, documentVisible])

  const refreshConnectionsManually = async () => {
    const result = await dispatch(refreshDeviceConnections('manual'))
    if (refreshDeviceConnections.rejected.match(result)) {
      if (!result.meta.condition) toast.error(t('devices.connectionRefresh.failed'))
      return
    }
    const refresh = result.payload
    if (refresh.status === 'complete' || refresh.status === 'list-failed') {
      const { report } = refresh
      const description = [
        report.errors > 0 ? t('devices.connectionRefresh.errors', { count: report.errors }) : null,
        refresh.status === 'list-failed' ? t('devices.connectionRefresh.listFailed') : null,
      ]
        .filter(Boolean)
        .join('\n')
      const notify = description ? toast.error : toast.success
      notify(t('devices.connectionRefresh.summary', { ...report }), {
        description,
      })
    }
  }

  useEffect(() => {
    const handler = (event: { topic: string; eventType: string; payload: unknown }) => {
      if (event.topic === 'network-recovery' && event.eventType === 'network-recovery.changed') {
        dispatch(fetchNetworkRecoveryStatus())
        return
      }
      if (event.topic !== 'peers') return
      if (event.eventType === 'peers.changed') {
        // The event carries the full member snapshot (same source as
        // GET /paired-devices), so apply it directly instead of firing a
        // redundant HTTP refetch on every presence flip (issue #1129).
        const payload = event.payload as PeersChangedPayload
        dispatch(setSpaceMembers(payload.peers.map(peerSnapshotToMember)))
      }
    }
    const unsub = daemonWs.subscribe(['peers', 'network-recovery'], handler)
    return unsub
  }, [dispatch, documentVisible])

  // ── mobile devices state ──────────────────────────────────────
  const {
    devices: mobileDevices,
    devicesError: mobileDevicesError,
    settings: mobileSettings,
    addDialogOpen,
    settingsSheetOpen,
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
  const [switchSpaceOpen, setSwitchSpaceOpen] = useState(false)
  const [addP2PDialogOpen, setAddP2PDialogOpen] = useState(false)
  const [{ targetId: unpairTargetId, busy: unpairBusy }, updateUnpair] = useReducer(unpairReducer, {
    targetId: null,
    busy: false,
  })
  const unpairDialogOpen = unpairTargetId !== null
  const unpairBusyRef = useRef(false)

  const handleUnpairRequest = (peerId: string) => {
    if (unpairBusyRef.current) return
    updateUnpair({ type: 'select', id: peerId })
  }

  const handleUnpairDialogOpenChange = (open: boolean) => {
    if (!open && unpairBusyRef.current) return
    if (!open) updateUnpair({ type: 'close' })
  }

  const handleUnpairConfirm = async () => {
    if (!unpairTargetId || unpairBusyRef.current) return
    unpairBusyRef.current = true
    updateUnpair({ type: 'start' })
    try {
      await unpairDevice(unpairTargetId)
      await refreshDeviceTrust()
      dispatch(fetchSpaceMembers())
      dispatch(fetchSpaceProtection())
      setSelection({ kind: 'local' })
      updateUnpair({ type: 'close' })
    } catch (error) {
      log.error({ err: error }, 'failed to remove device')
      toast.error(t('devices.memberRemoval.errors.removeFailed'))
    } finally {
      unpairBusyRef.current = false
      updateUnpair({ type: 'settled' })
    }
  }

  const getDeviceTrustStatusForPeer = (peerId: string) => {
    const relationship = trustListView.relationshipsByDeviceId.get(peerId)
    return relationship ? getDeviceTrustStatus(relationship, t)?.status : undefined
  }

  const unpairTargetDevice = peers.find(d => d.peerId === unpairTargetId)
  const networkRecoveryVisible =
    networkRecoveryError !== null || (networkRecovery !== null && networkRecovery.phase !== 'idle')
  const canRetryNetworkRecovery =
    networkRecovery?.phase === 'failed' &&
    networkRecovery.retryable &&
    networkRecoveryRequestId === null
  const requestNetworkRecoveryNow = () => {
    if (!canRetryNetworkRecovery) return
    void dispatch(requestNetworkRecovery())
  }

  return {
    selectionId,
    t,
    dispatch,
    now,
    localDevice,
    localDeviceLoading,
    localDeviceError,
    spaceMembersError,
    spaceProtectionError,
    networkRecovery,
    networkRecoveryError,
    manualRefreshInProgress,
    peers,
    onlineCount,
    localDeviceStatus,
    refreshDeviceTrust,
    refreshConnectionsManually,
    trustListView,
    mobileDevices,
    mobileDevicesError,
    mobileSettings,
    addDialogOpen,
    settingsSheetOpen,
    revokeTarget,
    revokeBusy,
    mobileActions,
    selection,
    setSelection,
    selectedPeer,
    selectedMobile,
    effectiveSelection,
    switchSpaceOpen,
    setSwitchSpaceOpen,
    addP2PDialogOpen,
    setAddP2PDialogOpen,
    unpairDialogOpen,
    unpairBusy,
    handleUnpairRequest,
    handleUnpairDialogOpenChange,
    handleUnpairConfirm,
    getDeviceTrustStatusForPeer,
    unpairTargetDevice,
    networkRecoveryVisible,
    canRetryNetworkRecovery,
    requestNetworkRecoveryNow,
  }
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
  revokeTarget: MobileDeviceView | null
  revokeBusy: boolean
  actions: {
    reload: () => void
    handleAddClick: () => void
    handleRevokeConfirm: () => Promise<void>
    requestRevoke: (device: MobileDeviceView) => void
    clearRevokeTarget: () => void
    setAddDialogOpen: (open: boolean) => void
    setSettingsSheetOpen: (open: boolean) => void
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

  // Preload settings for the selected mobile device panel.
  useEffect(() => {
    void reload()
    getMobileSyncSettings()
      .then(setSettings)
      .catch(err => {
        log.warn({ err }, 'failed to preload mobile sync settings')
      })
  }, [reload])

  const handleAddClick = useCallback(() => {
    setAddDialogOpen(true)
  }, [])

  const handleRevokeConfirm = useCallback(async () => {
    if (!revokeTarget) return
    setRevokeBusy(true)
    try {
      await revokeMobileDevice(revokeTarget.deviceId)
      toast.success(
        t('devices.mobileSync.revoke.confirmTitle', {
          label: revokeTarget.label,
        })
      )
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
    revokeTarget,
    revokeBusy,
    actions: {
      reload: () => void reload(),
      handleAddClick,
      handleRevokeConfirm,
      requestRevoke: setRevokeTarget,
      clearRevokeTarget: () => setRevokeTarget(null),
      setAddDialogOpen,
      setSettingsSheetOpen,
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
        return t('devices.mobileSync.errors.persistenceFailed', {
          message: e.message,
        })
      case 'SETTINGS_LOAD_FAILED':
        return t('devices.mobileSync.errors.settingsLoadFailed', {
          message: e.message,
        })
      default: {
        const message = (e as { message?: string }).message ?? e.code
        return t('devices.mobileSync.errors.unknown', { message })
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileSync.errors.unknown', { message })
}
