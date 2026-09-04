import { useEffect, useEffectEvent } from 'react'
import { getDeviceTrustSnapshot } from '@/api/daemon/device-trust'
import type { JoinSpaceResponse } from '@/api/daemon/setupV2'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'

const log = createLogger('join-admission')
const JOIN_STATUS_POLL_MS = 1000

export type JoinAdmissionResolution = Exclude<JoinSpaceResponse, { status: 'pending' }>

export function useJoinAdmission(
  joinId: string | null,
  onResolved: (result: JoinAdmissionResolution) => void
) {
  const refresh = useEffectEvent(async () => {
    if (!joinId) return
    try {
      const snapshot = await getDeviceTrustSnapshot()
      const currentJoin = snapshot.currentJoin
      if (currentJoin?.joinId === joinId && currentJoin.status !== 'pending') {
        onResolved(currentJoin)
      }
    } catch (err) {
      log.warn({ err, joinId }, 'failed to refresh durable admission')
    }
  })

  useEffect(() => {
    if (!joinId) return
    void refresh()
    const pollId = setInterval(() => void refresh(), JOIN_STATUS_POLL_MS)
    const unsubscribeDeviceTrust = daemonWs.subscribe(['device-trust', 'system'], event => {
      if (
        event.eventType === 'device-trust.changed' ||
        event.eventType === 'system.refresh_required'
      ) {
        void refresh()
      }
    })
    const unsubscribeReconnect = daemonWs.onReconnect(() => void refresh())
    return () => {
      clearInterval(pollId)
      unsubscribeDeviceTrust()
      unsubscribeReconnect()
    }
  }, [joinId])
}
