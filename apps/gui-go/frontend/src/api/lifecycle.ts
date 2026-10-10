/**
 * Lifecycle API — stable facade layer delegating to daemon HTTP endpoints.
 */

import {
  getLifecycleStatus as daemonGetLifecycleStatus,
  retryLifecycle as daemonRetryLifecycle,
} from '@/api/daemon/lifecycle'
import type { LifecycleStatusDto } from '@/api/types'

export async function getLifecycleStatus(): Promise<LifecycleStatusDto> {
  return daemonGetLifecycleStatus()
}

export async function retryLifecycle(): Promise<void> {
  return daemonRetryLifecycle()
}
