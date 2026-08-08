/**
 * Daemon member API module — typed HTTP client for per-member sync preferences.
 *
 * Phase 4b PR-3：取代 `./device.ts` 里读写 `PairedDevice.sync_settings` 的旧路径,
 * 对应后端 `MemberSyncPreferencesDto` (双向 send/receive + 双套 content types)。
 *
 * # Endpoints / 端点
 * - `GET /member/:device_id/sync-preferences`
 * - `PATCH /member/:device_id/sync-preferences`
 *
 * The device detail UI exposes both directions. Updates remain partial so the
 * daemon and Engine own merge semantics and the authoritative full value.
 */

import {
  continueMemberRemoval as continueMemberRemovalSdk,
  getCurrentMemberRemoval as getCurrentMemberRemovalSdk,
  getMembershipConvergence as getMembershipConvergenceSdk,
  getMemberSyncPreferences as getMemberSyncPreferencesSdk,
  getSharedDeviceRefresh as getSharedDeviceRefreshSdk,
  getSpaceProtection as getSpaceProtectionSdk,
  secureRemoveLegacyMember as secureRemoveLegacyMemberSdk,
  startSharedDeviceRefresh as startSharedDeviceRefreshSdk,
  updateMemberSyncPreferences as updateMemberSyncPreferencesSdk,
} from '@/api/generated/sdk.gen'
import type {
  LegacyBootstrapDto,
  MemberRemovalDto,
  MemberSyncPreferencesPatchDto,
  MembershipConvergenceDto,
  SecureLegacyRemovalDto,
  SpaceProtectionDto,
} from '@/api/generated/types.gen'
import { daemonClient } from './client'
import { DaemonApiError } from './errors'

// ── Value objects ───────────────────────────────────────────────

/** Content type toggles. Matches `ContentTypesDto` on the Rust side. */
export interface ContentTypes {
  text: boolean
  image: boolean
  link: boolean
  file: boolean
  codeSnippet: boolean
  richText: boolean
}

/** Partial content type toggles for PATCH. */
export interface ContentTypesPatch {
  text?: boolean
  image?: boolean
  link?: boolean
  file?: boolean
  codeSnippet?: boolean
  richText?: boolean
}

/**
 * Sync preferences recorded for a space member.
 * Matches `MemberSyncPreferencesDto` on the Rust side.
 */
export interface MemberSyncPreferences {
  sendEnabled: boolean
  receiveEnabled: boolean
  sendContentTypes: ContentTypes
  receiveContentTypes: ContentTypes
}

/**
 * Partial member sync preferences for PATCH.
 * Any omitted top-level field keeps its current value server-side.
 */
export interface MemberSyncPreferencesPatch {
  sendEnabled?: boolean
  receiveEnabled?: boolean
  sendContentTypes?: ContentTypesPatch
  receiveContentTypes?: ContentTypesPatch
}

export type SpaceProtectionMode = 'legacy' | 'migrating' | 'ready'
export type MemberProtectionStatus =
  | 'legacy_unprotected'
  | 'protected'
  | 'awaiting_readmission'
  | 'requires_readmission'
  | 'recovery_required'
export type LegacyBootstrapOutcome = 'awaiting_readmission' | 'complete' | 'recovery_required'
export type MembershipConvergenceState =
  | 'complete'
  | 'converging'
  | 'waiting_for_upgrade'
  | 'blocked'

export type SharedDeviceRefreshPhase = 'started' | 'discovering' | 'connecting' | 'round_completed'
export type SharedDeviceRefreshDeviceState =
  | 'discovered'
  | 'connecting'
  | 'connected'
  | 'already_present'
  | 'waiting_for_peer'
  | 'waiting_for_update'
  | 'version_incompatible'
  | 'rejected'

export interface SharedDeviceRefreshDevice {
  deviceId: string
  displayName: string
  state: SharedDeviceRefreshDeviceState
}

/** Complete Engine-owned result for one shared-device refresh request. */
export interface SharedDeviceRefreshSnapshot {
  requestId: string
  phase: SharedDeviceRefreshPhase
  devices: SharedDeviceRefreshDevice[]
  totalCount: number
  discoveredCount: number
  connectingCount: number
  connectedCount: number
  alreadyPresentCount: number
  waitingForPeerCount: number
  waitingForUpdateCount: number
  versionIncompatibleCount: number
  rejectedCount: number
  unavailableSourceCount: number
}

export interface LegacyBootstrap {
  bootstrapId: string
  outcome: LegacyBootstrapOutcome
  pendingReadmission: number
}

export interface MemberProtection {
  deviceId: string
  status: MemberProtectionStatus
}

/** Engine-authoritative protection snapshot for the active space. */
export interface SpaceProtection {
  mode: SpaceProtectionMode
  members: MemberProtection[]
  legacyBootstrap: LegacyBootstrap | null
}

/** Engine-authoritative connection state for the active space. */
export interface MembershipConvergence {
  state: MembershipConvergenceState
}

/** Result of beginning an Engine-owned secure Legacy member removal. */
export interface SecureLegacyRemoval {
  bootstrap: LegacyBootstrap
}

export type MemberRemovalOutcome =
  | 'local_only'
  | 'recovering'
  | 'applied'
  | 'complete'
  | 'recovery_required'

/** Engine-owned removal progress for the active space. */
export interface MemberRemoval {
  revocationId: string | null
  outcome: MemberRemovalOutcome
  pendingRecipients: number
  removedDeviceIds: string[]
  pendingRecipientDeviceIds: string[]
  updatedAtMs: number
}

/** Starts a shared-device refresh round and returns its Engine-owned request ID. */
export async function startSharedDeviceRefresh(): Promise<string> {
  const started = await daemonClient.callEnveloped(() =>
    startSharedDeviceRefreshSdk({ throwOnError: true })
  )
  return started.requestId
}

/** Reads the complete current snapshot for one shared-device refresh request. */
export async function getSharedDeviceRefresh(
  requestId: string
): Promise<SharedDeviceRefreshSnapshot> {
  const data = await daemonClient.callEnveloped(() =>
    getSharedDeviceRefreshSdk({ path: { request_id: requestId }, throwOnError: true })
  )
  return data as unknown as SharedDeviceRefreshSnapshot
}

/** True when the daemon reports that the refresh request no longer exists. */
export function isSharedDeviceRefreshNotFound(error: unknown): boolean {
  if (!(error instanceof DaemonApiError)) return false
  if (error.details && typeof error.details === 'object') {
    return (error.details as { code?: unknown }).code === 'shared_device_refresh_not_found'
  }
  return false
}

/** Reads the Engine-owned removal that remains active across daemon restarts. */
export async function getCurrentMemberRemoval(): Promise<MemberRemoval | null> {
  const envelope = await daemonClient.callSdk(() =>
    getCurrentMemberRemovalSdk({ throwOnError: true })
  )
  return envelope.data ? toMemberRemoval(envelope.data) : null
}

/** Continues an active removal after the user explicitly confirms permanent loss. */
export async function continueMemberRemoval(
  revocationId: string,
  permanentlyLostDeviceIds: string[]
): Promise<MemberRemoval> {
  const removal = await daemonClient.callEnveloped(() =>
    continueMemberRemovalSdk({
      body: { revocationId, permanentlyLostDeviceIds },
      throwOnError: true,
    })
  )
  return toMemberRemoval(removal)
}

// ── Public API ──────────────────────────────────────────────────

export async function getMemberSyncPreferences(deviceId: string): Promise<MemberSyncPreferences> {
  // Route through the generated SDK; `callEnveloped` unwraps the SDK's `{ data }`
  // envelope down to the preferences payload. The generated
  // `MemberSyncPreferencesDto` is structurally equivalent to the hand-written
  // `MemberSyncPreferences` (camelCase wire fields), bridged here to keep the
  // public return type stable for downstream consumers.
  const data = await daemonClient.callEnveloped(() =>
    getMemberSyncPreferencesSdk({ path: { device_id: deviceId }, throwOnError: true })
  )
  return data as unknown as MemberSyncPreferences
}

export async function getSpaceProtection(): Promise<SpaceProtection> {
  const data: SpaceProtectionDto = await daemonClient.callEnveloped(() =>
    getSpaceProtectionSdk({ throwOnError: true })
  )
  return {
    mode: data.mode,
    members: data.members.map(member => ({ deviceId: member.deviceId, status: member.status })),
    legacyBootstrap: data.legacyBootstrap ? toLegacyBootstrap(data.legacyBootstrap) : null,
  }
}

export async function getMembershipConvergence(): Promise<MembershipConvergence> {
  const data: MembershipConvergenceDto = await daemonClient.callEnveloped(() =>
    getMembershipConvergenceSdk({ throwOnError: true })
  )
  return { state: data.state }
}

export async function secureRemoveLegacyMember(deviceId: string): Promise<SecureLegacyRemoval> {
  const data: SecureLegacyRemovalDto = await daemonClient.callEnveloped(() =>
    secureRemoveLegacyMemberSdk({ path: { device_id: deviceId }, throwOnError: true })
  )
  return { bootstrap: toLegacyBootstrap(data.bootstrap) }
}

function toLegacyBootstrap(bootstrap: LegacyBootstrapDto): LegacyBootstrap {
  return {
    bootstrapId: bootstrap.bootstrapId,
    outcome: bootstrap.outcome,
    pendingReadmission: bootstrap.pendingReadmission,
  }
}

export function toMemberRemoval(removal: MemberRemovalDto): MemberRemoval {
  return {
    revocationId: removal.revocationId ?? null,
    outcome: removal.outcome,
    pendingRecipients: removal.pendingRecipients,
    removedDeviceIds: removal.removedDeviceIds,
    pendingRecipientDeviceIds: removal.pendingRecipientDeviceIds,
    updatedAtMs: removal.updatedAtMs,
  }
}

/** True only when the Engine refused an unsafe Legacy local-only removal. */
export function isLegacyBootstrapRequired(error: unknown): boolean {
  if (!(error instanceof DaemonApiError) || !error.details || typeof error.details !== 'object') {
    return false
  }
  return (error.details as { code?: unknown }).code === 'legacy_bootstrap_required'
}

/** True when the Engine requires the current removal to be shown before another can begin. */
export function isMemberRemovalBlocked(error: unknown): boolean {
  if (!(error instanceof DaemonApiError) || !error.details || typeof error.details !== 'object') {
    return false
  }
  const code = (error.details as { code?: unknown }).code
  return code === 'member_removal_in_progress' || code === 'member_removal_recovery_required'
}

export async function updateMemberSyncPreferences(
  deviceId: string,
  patch: MemberSyncPreferencesPatch
): Promise<MemberSyncPreferences> {
  // PATCH returns only `{ data: { success } }` (ApiEnvelope<MemberSyncResultDto>);
  // callSdk throws on non-2xx, so reaching here means success. Re-fetch to return
  // the authoritative merged preferences (preserves the Promise<MemberSyncPreferences>
  // contract that devicesSlice stores into state — the PATCH body no longer echoes it).
  await daemonClient.callSdk(() =>
    updateMemberSyncPreferencesSdk({
      path: { device_id: deviceId },
      // `MemberSyncPreferencesPatch` is structurally equivalent to the generated
      // `MemberSyncPreferencesPatchDto`; bridge for the SDK body param.
      body: patch as unknown as MemberSyncPreferencesPatchDto,
      throwOnError: true,
    })
  )
  return getMemberSyncPreferences(deviceId)
}

// ── Default-value helpers (used by "restore defaults" button) ──

/**
 * Mirror of `MemberSyncPreferences::default()` on the Rust side (all toggles on).
 * Used by Restore Defaults to push an explicit reset without relying on a
 * server-side "null clears overrides" semantic (which does not exist for
 * `space_member`; every member always has a preferences record).
 */
export const DEFAULT_SEND_CONTENT_TYPES: ContentTypes = {
  text: true,
  image: true,
  link: true,
  file: true,
  codeSnippet: true,
  richText: true,
}
