/** How one content type syncs with a peer, as the per-type pop-up offers it. */
export type SyncDirection = 'both' | 'send_only' | 'receive_only' | 'off'

/** The pop-up's order. */
export const SYNC_DIRECTIONS: readonly SyncDirection[] = [
  'both',
  'send_only',
  'receive_only',
  'off',
]

export const DIRECTION_VALUES: Record<SyncDirection, { send: boolean; receive: boolean }> = {
  both: { send: true, receive: true },
  send_only: { send: true, receive: false },
  receive_only: { send: false, receive: true },
  off: { send: false, receive: false },
}

export function getSyncDirection(send: boolean, receive: boolean): SyncDirection {
  if (send && receive) return 'both'
  if (send) return 'send_only'
  if (receive) return 'receive_only'
  return 'off'
}
