import {
  getNetworkRecoveryStatus as getNetworkRecoveryStatusSdk,
  recoverNetwork as recoverNetworkSdk,
} from '@/api/generated/sdk.gen'
import type { NetworkRecoveryStatusResponse } from '@/api/generated/types.gen'
import { daemonClient } from './client'

export type NetworkRecoveryPhase = 'idle' | 'recovering' | 'retryScheduled' | 'failed'

export interface NetworkRecoveryStatus {
  phase: NetworkRecoveryPhase
  retryable: boolean
  nextRetryInMs?: number
}

function toNetworkRecoveryStatus(data: NetworkRecoveryStatusResponse): NetworkRecoveryStatus {
  return data as NetworkRecoveryStatus
}

export async function getNetworkRecoveryStatus(): Promise<NetworkRecoveryStatus> {
  const data = await daemonClient.callEnveloped(() =>
    getNetworkRecoveryStatusSdk({ throwOnError: true })
  )
  return toNetworkRecoveryStatus(data)
}

export async function recoverNetwork(): Promise<NetworkRecoveryStatus> {
  const data = await daemonClient.callEnveloped(() => recoverNetworkSdk({ throwOnError: true }))
  return toNetworkRecoveryStatus(data)
}
