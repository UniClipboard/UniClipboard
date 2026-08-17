import { useEffect, useEffectEvent } from 'react'
import { getDeviceTrust } from '@/api/daemon/device-trust'
import type { JoinSpaceResponse } from '@/api/daemon/setupV2'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'

const log = createLogger('join-admission')

export function useJoinAdmission(
  joinId: string | null,
  onResolved: (result: Exclude<JoinSpaceResponse, { status: 'pending' }>) => void
) {
  const refresh = useEffectEvent(async () => {
    if (!joinId) return
    try {
      const currentJoin = (await getDeviceTrust()).currentJoin
      if (!currentJoin || currentJoin.joinId !== joinId || currentJoin.status === 'pending') return
      onResolved(currentJoin)
    } catch (err) {
      log.warn({ err, joinId }, 'failed to refresh durable admission')
    }
  })

  useEffect(() => {
    if (!joinId) return
    void refresh()
    const unsubscribeDeviceTrust = daemonWs.subscribe(['device-trust'], event => {
      if (event.eventType === 'device-trust.changed') void refresh()
    })
    const unsubscribeReconnect = daemonWs.onReconnect(() => void refresh())
    return () => {
      unsubscribeDeviceTrust()
      unsubscribeReconnect()
    }
  }, [joinId])
}
