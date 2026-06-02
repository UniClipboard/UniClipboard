/**
 * Members API — typed accessors for daemon space-member endpoints.
 *
 * Replaces the libp2p-era pairing.ts wrappers (Slice 4 P5a-2). The
 * backend endpoints are unchanged; only the frontend module name and
 * type shape are. SpaceMember is now aligned with the daemon
 * `SpaceMemberDto` and no longer carries the mDNS-era
 * `sharedSecret`/`pairedAt`/`lastSeen`/`lastKnownAddresses` fields.
 *
 * # Transport / 传输 (ADR-008 P7)
 * The three wrappers route through the @hey-api generated SDK
 * (`getLocalDeviceInfo` / `listPairedDevices` / `unpairDevice`) via
 * `daemonClient.callSdk`, which drives the daemon session lifecycle and
 * normalizes thrown SDK errors back into the shared `DaemonApiError` shape.
 * Every endpoint returns the canonical `ApiEnvelope { data, ts }`; the public
 * wrapper signatures and the hand-written domain types below are preserved
 * verbatim for downstream consumers, with the generated DTOs bridged at the
 * boundary.
 */

import {
  getLocalDeviceInfo as getLocalDeviceInfoSdk,
  listPairedDevices as listPairedDevicesSdk,
  unpairDevice as unpairDeviceSdk,
} from '@/api/generated/sdk.gen'
import { daemonClient } from './client'

export interface LocalDeviceInfo {
  peerId: string
  deviceName: string
}

/**
 * 连接通道 4 态 wire 字符串 —— Phase 96 INDIC-01。
 *
 * 取值由后端 `connection_channel_to_wire`（uc-application/facade/roster/mod.rs）
 * 单点产出，前端按字符串模式匹配渲染徽章。**禁止**自行扩展取值；新增态需
 * 同步改 Rust enum + wire 映射 + 本类型 + 渲染分支 + i18n key。
 */
export type ConnectionChannel = 'direct' | 'relay' | 'offline' | 'unknown'

/**
 * Space member — matches `SpaceMemberDto` on the Rust side.
 *
 * `connected` 来自 `IrohPresenceAdapter.last_state`，由 ensure_reachable
 * 拨号成功 / `connection.closed()` watchdog 维护；`/paired-devices` 通过
 * `list_peer_snapshots` 聚合 `PresencePort.current_state()` 返回真实值。
 *
 * `channel` 是 Phase 96 INDIC-01 新增字段：连接通道 4 态。"Out of LAN"
 * 灰态由 UI 基于 `channel + LAN-only setting` 合成，不在 wire 协议里。
 */
export interface SpaceMember {
  peerId: string
  deviceName: string
  pairingState: string
  lastSeenAtMs: number | null
  connected: boolean
  channel: ConnectionChannel
  connectionAddress: string | null
}

/**
 * Get the local device's peer id + resolved device name.
 *
 * 获取本地设备信息（peer ID + 解析后的设备名称）。
 */
export async function getLocalDeviceInfo(): Promise<LocalDeviceInfo> {
  // `callSdk` unwraps the SDK's `{ data }` to the `LocalDeviceInfoEnvelope`,
  // then we unwrap `.data` to the payload. The generated `LocalDeviceInfoDto`
  // is structurally equivalent to the hand-written `LocalDeviceInfo`, bridged
  // here to keep the public return type stable for consumers.
  const envelope = await daemonClient.callSdk(() => getLocalDeviceInfoSdk({ throwOnError: true }))
  return envelope.data as unknown as LocalDeviceInfo
}

/**
 * Get the list of admitted space members.
 *
 * 获取已配对的设备列表。
 */
export async function getPairedPeers(): Promise<SpaceMember[]> {
  // Backed by the `listPairedDevices` SDK fn (GET /paired-devices). `callSdk`
  // unwraps the SDK's `{ data }` to the `SpaceMemberListEnvelope`, then `.data`
  // is the `SpaceMemberDto[]` payload, bridged to the hand-written `SpaceMember[]`.
  const envelope = await daemonClient.callSdk(() => listPairedDevicesSdk({ throwOnError: true }))
  return envelope.data as unknown as SpaceMember[]
}

/**
 * Alias of {@link getPairedPeers} kept for source-level compatibility
 * with consumers that previously distinguished "with status".
 */
export async function getPairedPeersWithStatus(): Promise<SpaceMember[]> {
  return getPairedPeers()
}

/**
 * Remove a paired device from the local member registry.
 *
 * 取消配对：从本机成员仓库移除该设备。
 */
export async function unpairDevice(peerId: string): Promise<void> {
  // POST /pairing/unpair — 204 No Content, so do NOT read `.data`. The
  // `peerId` goes in the request body (`UnpairDeviceRequest`).
  await daemonClient.callSdk(() => unpairDeviceSdk({ body: { peerId }, throwOnError: true }))
}
