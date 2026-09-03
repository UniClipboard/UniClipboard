import { useEffect, useEffectEvent } from 'react'
import { getDeviceTrustSnapshot } from '@/api/daemon/device-trust'
import type { ActiveJoinSpaceResponse, JoinSpaceRejectionReason } from '@/api/daemon/setupV2'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'

const log = createLogger('join-admission')

export type JoinAdmissionResolution =
  | {
      status: 'active'
      peerDeviceId: string | null
      result: ActiveJoinSpaceResponse | null
    }
  | { status: 'rejected'; reason: JoinSpaceRejectionReason }

export function useJoinAdmission(
  joinId: string | null,
  initialDeviceIds: ReadonlySet<string> | null,
  onResolved: (result: JoinAdmissionResolution) => void
) {
  const refresh = useEffectEvent(async () => {
    if (!joinId) return
    try {
      const snapshot = await getDeviceTrustSnapshot()
      const currentJoin = snapshot.currentJoin
      if (currentJoin?.joinId === joinId && currentJoin.status !== 'pending') {
        onResolved(
          currentJoin.status === 'active'
            ? {
                status: 'active',
                peerDeviceId: currentJoin.joinedSpace.sponsorDeviceId,
                result: currentJoin,
              }
            : { status: 'rejected', reason: currentJoin.reason }
        )
        return
      }
      if (initialDeviceIds === null || snapshot.localMembership !== 'active') return
      for (const device of snapshot.devices) {
        if (
          !device.isLocal &&
          device.membership === 'active' &&
          !initialDeviceIds.has(device.deviceId)
        ) {
          onResolved({ status: 'active', peerDeviceId: device.deviceId, result: null })
          return
        }
      }
    } catch (err) {
      log.warn({ err, joinId }, 'failed to refresh durable admission')
    }
  })

  useEffect(() => {
    if (!joinId) return
    void refresh()
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
      unsubscribeDeviceTrust()
      unsubscribeReconnect()
    }
  }, [joinId, initialDeviceIds])
}
