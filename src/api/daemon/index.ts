export { daemonClient } from './client'
export type { RequestOptions } from './client'
export type { DaemonConfig, SessionToken } from './types'
export { isSessionExpired } from './types'
export { DaemonApiError, DaemonErrorCode, mapStatusToErrorCode } from './errors'
export { signalLifecycleReady, getLifecycleStatus, retryLifecycle } from './lifecycle'
export { getSettings, updateSettings } from './settings'
export { exportLogs, getDebugStatus, updateDebugMode } from './diagnostics'
export type { DebugStatus, LogExportResult, UpdateDebugModeResult } from './diagnostics'
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
  initializeSpace,
  issuePairingInvitation,
  redeemInvitation,
  cancelInvitation,
  resetSetup,
  switchSpace,
  queryMigrationProgress,
  SetupV2Error,
} from './setupV2'
export type {
  CurrentInvitation,
  InitializeSpaceErrorKind,
  InitializeSpaceRequest,
  InitializeSpaceResponse,
  IssueInvitationErrorKind,
  IssueInvitationResponse,
  MigrationPhase,
  MigrationProgressResponse,
  QueryMigrationProgressErrorKind,
  RedeemInvitationErrorKind,
  RedeemRequest,
  RedeemResponse,
  SetupStateResponse,
  SwitchSpaceErrorKind,
  SwitchSpaceRequest,
  SwitchSpaceResponse,
} from './setupV2'
export {
  getLocalDeviceInfo,
  getPairedPeers,
  getPairedPeersWithStatus,
  unpairDevice,
} from './members'
export type { LocalDeviceInfo, SpaceMember } from './members'
export { refreshPresence } from './presence'
export type { PresenceRefreshResult } from './presence'
export { classifyPairingError } from './events'
export type { PairingErrorKind } from './events'
export { querySearch, getSearchStatus, getSearchTags, triggerSearchRebuild } from './search'
export type {
  SearchResultDto,
  SearchQueryResponse,
  SearchParams,
  SearchStatusData,
  SearchStatusResponse,
  SearchTagDto,
  SearchTagsResponse,
} from './search'
export { getUpgradeStatus, acknowledgeUpgrade } from './upgrade'
export type { UpgradeStatus } from './upgrade'
