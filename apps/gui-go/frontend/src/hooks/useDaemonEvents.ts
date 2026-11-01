import { useEffect, useRef } from 'react'
import { daemonWs } from '@/lib/daemon-ws'
import type { DaemonWsEvent } from '@/lib/daemon-ws'

/**
 * One peer entry in a `peers.changed` snapshot. Mirrors the daemon
 * `PeerSnapshotDto` (camelCase wire): the full member record, not just the
 * volatile presence fields, so the frontend can rebuild its member list from
 * the event alone instead of refetching `/paired-devices`.
 */
export interface PeerSnapshotPayloadItem {
  peerId: string
  deviceName?: string | null
  addresses?: string[]
  isPaired?: boolean
  connected: boolean
  pairingState?: string
  /** Phase 96 INDIC-01: 连接通道 4 态 wire 字符串。 */
  channel?: 'direct' | 'relay' | 'offline' | 'unknown'
  /** 当前活跃连接地址；直连=对端 IP:port，中转=relay 地址。 */
  connectionAddress?: string | null
}

/** Payload for `peers.changed` events — a full snapshot of the peer list. */
export interface PeersChangedPayload {
  peers: PeerSnapshotPayloadItem[]
}

/** Subscribe to encryption readiness notifications. */
export function useEncryptionState(onReady: () => void): void {
  const onReadyRef = useRef(onReady)
  // eslint-disable-next-line react-hooks/refs -- intentional: ref updates stabilize callbacks without re-running effect
  onReadyRef.current = onReady

  useEffect(() => {
    const handler = (event: DaemonWsEvent) => {
      if (event.topic !== 'encryption') return

      if (event.eventType === 'encryption.session_ready') {
        onReadyRef.current()
      }
    }

    const unsubscribe = daemonWs.subscribe(['encryption'], handler)
    return unsubscribe
  }, [])
}
