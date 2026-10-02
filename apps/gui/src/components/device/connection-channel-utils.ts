import type { ConnectionChannel } from '@/api/daemon/members'

export type DerivedBadgeKind = 'lan' | 'relay' | 'offline' | 'unknown' | 'outOfLan'
export type PeerStatusTone = 'success' | 'info' | 'off'

/**
 * `channel + lanOnlyActive` ⇒ 5 derived UI states. Pure function for testability.
 *
 * * `direct` ⇒ `lan` (independent of LAN-only setting)
 * * `relay`  ⇒ `outOfLan` when LAN-only is ON, otherwise `relay`
 * * `offline` ⇒ `outOfLan` when LAN-only is ON, otherwise `offline`
 * * `unknown` ⇒ always `unknown`
 */
export function deriveBadgeKind(
  channel: ConnectionChannel,
  lanOnlyActive: boolean
): DerivedBadgeKind {
  switch (channel) {
    case 'direct':
      return 'lan'
    case 'relay':
      return lanOnlyActive ? 'outOfLan' : 'relay'
    case 'offline':
      return lanOnlyActive ? 'outOfLan' : 'offline'
    case 'unknown':
    default:
      return 'unknown'
  }
}

export function derivePeerStatusTone(
  channel: ConnectionChannel,
  connected: boolean
): PeerStatusTone {
  if (!connected) return 'off'

  switch (channel) {
    case 'direct':
      return 'success'
    case 'relay':
      return 'info'
    case 'offline':
    case 'unknown':
    default:
      return 'off'
  }
}
