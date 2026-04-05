/**
 * Pairing API module — typed accessors for daemon pairing endpoints.
 *
 * Pairing API 模块 — daemon 配对端点的类型化访问器。
 *
 * # Endpoints / 端点
 * - `GET /peers` → discovered P2P peers
 * - `GET /paired-devices` → paired devices list
 * - `POST /pairing/initiate` → initiate pairing with a peer (body: `{ peerId }`)
 * - `POST /pairing/accept` → accept incoming pairing (body: `{ sessionId }`)
 * - `POST /pairing/reject` → reject incoming pairing (body: `{ sessionId }`)
 * - `POST /pairing/sessions/{sessionId}/verify` → verify pairing PIN (body: `{ pinMatches }`)
 * - `POST /pairing/unpair` → unpair a device (body: `{ peerId }`)
 *
 * These replace the Tauri `invoke()` calls in `p2p.ts`, enabling the GUI to
 * operate independently of the Tauri layer once the daemon is reachable.
 */

import { daemonClient } from './client'
import type { RequestOptions } from './client'

// ── Types (defined here to avoid circular imports with p2p.ts facade) ─

export interface P2PPeerInfo {
  peerId: string
  deviceName?: string | null
  addresses: string[]
  isPaired: boolean
  connected: boolean
}

export interface LocalDeviceInfo {
  peerId: string
  deviceName: string
}

interface LocalDeviceInfoResponse {
  data: LocalDeviceInfo
  ts: number
}

export interface PairedPeer {
  peerId: string
  deviceName: string
  sharedSecret: number[]
  pairedAt: string
  lastSeen: string | null
  lastKnownAddresses: string[]
  connected: boolean
}

export interface P2PPairingRequest {
  peerId: string
}

export interface P2PPairingResponse {
  sessionId: string
  success: boolean
  error?: string
}

export interface P2PPinVerifyRequest {
  sessionId: string
  pinMatches: boolean
}

export type P2PPairingVerificationKind =
  | 'request'
  | 'verification'
  | 'verifying'
  | 'complete'
  | 'failed'

export type PairingErrorKind =
  | 'active_session_exists'
  | 'no_local_participant'
  | 'session_not_found'
  | 'daemon_unavailable'
  | 'unknown'

export interface P2PPairingVerificationEvent {
  sessionId: string
  kind: P2PPairingVerificationKind
  peerId?: string
  deviceName?: string
  code?: string
  localFingerprint?: string
  peerFingerprint?: string
  error?: string
}

export interface P2PPeerConnectionEvent {
  peerId: string
  deviceName?: string | null
  connected: boolean
}

export interface P2PPeerNameUpdatedEvent {
  peerId: string
  deviceName: string
}

export interface P2PPeerDiscoveryChangedEvent {
  peerId: string
  deviceName?: string | null
  addresses: string[]
  discovered: boolean
}

export interface ContentTypes {
  text: boolean
  image: boolean
  link: boolean
  file: boolean
  codeSnippet: boolean
  richText: boolean
}

export interface SyncSettings {
  autoSync: boolean
  syncFrequency: 'realtime' | 'interval'
  contentTypes: ContentTypes
}

export { classifyPairingError } from '@/api/daemon/events'

// ── Error helper ─────────────────────────────────────────────────

function stringifyPairingError(error: unknown): string {
  if (typeof error === 'string') {
    return error
  }
  if (error instanceof Error) {
    return error.message
  }
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof (error as { message: unknown }).message === 'string'
  ) {
    return (error as { message: string }).message
  }
  return String(error)
}

// ── Public API ────────────────────────────────────────────────────

/**
 * Get discovered P2P peers.
 *
 * 获取发现的 P2P 设备列表。
 */
export async function getP2PPeers(): Promise<P2PPeerInfo[]> {
  return daemonClient.request<P2PPeerInfo[]>('/peers')
}

/**
 * Get paired devices list.
 *
 * 获取已配对的设备列表。
 */
export async function getPairedPeers(): Promise<PairedPeer[]> {
  return daemonClient.request<PairedPeer[]>('/paired-devices')
}

/**
 * Get paired devices list (with connection status).
 *
 * 获取已配对的设备列表（带连接状态）。
 */
export async function getPairedPeersWithStatus(): Promise<PairedPeer[]> {
  return daemonClient.request<PairedPeer[]>('/paired-devices')
}

/**
 * Get local device info (peer ID + resolved device name).
 *
 * 获取本地设备信息（peer ID + 解析后的设备名称）。
 */
export async function getLocalDeviceInfo(): Promise<LocalDeviceInfo> {
  const response = await daemonClient.request<LocalDeviceInfoResponse>('/device/me')
  return response.data
}

/**
 * Initiate P2P pairing with a peer.
 * Returns `{ success: false, error }` on failure to match the existing contract.
 */
export async function initiateP2PPairing(request: P2PPairingRequest): Promise<P2PPairingResponse> {
  try {
    return await daemonClient.request<P2PPairingResponse>('/pairing/initiate', {
      method: 'POST',
      body: { peerId: request.peerId },
    })
  } catch (error) {
    return {
      sessionId: '',
      success: false,
      error: stringifyPairingError(error),
    }
  }
}

/**
 * Accept incoming P2P pairing (receiver side).
 *
 * 接受 P2P 配对请求（接收方）。
 */
export async function acceptP2PPairing(sessionId: string): Promise<void> {
  const options: RequestOptions = {
    method: 'POST',
    body: { sessionId },
  }
  await daemonClient.request<void>('/pairing/accept', options)
}

/**
 * Reject incoming P2P pairing request.
 * Note: peerId parameter is kept for signature compatibility but only sessionId
 * is sent to the HTTP endpoint.
 *
 * 拒绝 P2P 配对请求。
 */
export async function rejectP2PPairing(sessionId: string, _peerId: string): Promise<void> {
  const options: RequestOptions = {
    method: 'POST',
    body: { sessionId },
  }
  await daemonClient.request<void>('/pairing/reject', options)
}

/**
 * Verify pairing PIN and complete pairing.
 *
 * 验证 PIN 并完成配对。
 */
export async function verifyP2PPairingPin(sessionId: string, pinMatches: boolean): Promise<void> {
  const options: RequestOptions = {
    method: 'POST',
    body: { pinMatches },
  }
  await daemonClient.request<void>(`/pairing/sessions/${sessionId}/verify`, options)
}

/**
 * Unpair a P2P device.
 *
 * 取消 P2P 配对连接。
 */
export async function unpairP2PDevice(peerId: string): Promise<void> {
  const options: RequestOptions = {
    method: 'POST',
    body: { peerId },
  }
  await daemonClient.request<void>('/pairing/unpair', options)
}
