/**
 * Members API — typed accessors for daemon space-member endpoints.
 *
 * Replaces the libp2p-era pairing.ts wrappers (Slice 4 P5a-2). The
 * backend endpoints are unchanged; only the frontend module name and
 * type shape are. SpaceMember is now aligned with the daemon
 * `SpaceMemberDto` and no longer carries the mDNS-era
 * `sharedSecret`/`pairedAt`/`lastSeen`/`lastKnownAddresses` fields.
 */

import { daemonClient } from './client'

export interface LocalDeviceInfo {
  peerId: string
  deviceName: string
}

interface LocalDeviceInfoResponse {
  data: LocalDeviceInfo
  ts: number
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
  const response = await daemonClient.request<LocalDeviceInfoResponse>('/device/me')
  return response.data
}

/**
 * Get the list of admitted space members.
 *
 * 获取已配对的设备列表。
 */
export async function getPairedPeers(): Promise<SpaceMember[]> {
  return daemonClient.request<SpaceMember[]>('/paired-devices')
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
  await daemonClient.request<void>('/pairing/unpair', {
    method: 'POST',
    body: { peerId },
  })
}
