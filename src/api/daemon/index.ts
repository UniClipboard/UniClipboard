export { daemonClient } from './client'
export type { RequestOptions } from './client'
export type { DaemonConfig, SessionToken } from './types'
export { isSessionExpired } from './types'
export { DaemonApiError, DaemonErrorCode, mapStatusToErrorCode } from './errors'
export { signalLifecycleReady, getLifecycleStatus, retryLifecycle } from './lifecycle'
export { getSettings, updateSettings } from './settings'
export type {
  Settings,
  GeneralSettings,
  SyncSettings,
  SecuritySettings,
  PairingSettings,
  FileSyncSettings,
  ContentTypes,
  RetentionPolicy,
  RetentionRule,
  ShortcutKey,
  Theme,
  UpdateChannel,
  SyncFrequency,
  RuleEvaluation,
} from './settings'
export { getEncryptionState, unlockEncryption, lockEncryption } from './encryption'
export type { EncryptionStateResponse } from './encryption'
export {
  getClipboardEntries,
  getClipboardEntry,
  deleteClipboardEntry,
  restoreClipboardEntry,
  toggleFavorite,
  getClipboardStats,
  getClipboardEntryResource,
} from './clipboard'
export type {
  ClipboardEntryDto,
  ClipboardEntriesResponse,
  ClipboardStats,
  ClipboardEntryResource,
  RestoreResult,
} from './clipboard'
export { getStorageStats, clearCache } from './storage'
export type { StorageStats } from './storage'
export {
  getSetupState,
  startNewSpace,
  startJoinSpace,
  selectJoinPeer,
  submitPassphrase,
  verifyPassphrase,
  confirmPeerTrust,
  cancelSetup,
} from './setup'
export type { SetupError, SetupState } from './setup'
export {
  getP2PPeers,
  getPairedPeers,
  getPairedPeersWithStatus,
  initiateP2PPairing,
  acceptP2PPairing,
  rejectP2PPairing,
  verifyP2PPairingPin,
  unpairP2PDevice,
} from './pairing'
export type {
  P2PPeerInfo,
  LocalDeviceInfo,
  PairedPeer,
  P2PPairingRequest,
  P2PPairingResponse,
  P2PPinVerifyRequest,
  P2PPairingVerificationKind,
  PairingErrorKind,
  P2PPairingVerificationEvent,
  P2PPeerConnectionEvent,
  P2PPeerNameUpdatedEvent,
  P2PPeerDiscoveryChangedEvent,
} from './pairing'
export type { SpaceAccessCompletedEvent } from './setup'
export { querySearch, getSearchStatus, triggerSearchRebuild } from './search'
export type {
  SearchResultDto,
  SearchQueryResponse,
  SearchParams,
  SearchStatusData,
  SearchStatusResponse,
} from './search'
