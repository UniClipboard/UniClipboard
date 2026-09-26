import { DaemonApiError } from '@/api/daemon/errors'

/**
 * Operation-local reasons why the device group query failed. Only
 * `unavailable` is fixed by querying again; unlock and recovery are not.
 */
export type DeviceGroupChoicesFailure = 'unavailable' | 'unlock_required' | 'recovery_required'

export function deviceGroupChoicesFailure(error: unknown): DeviceGroupChoicesFailure {
  const body = error instanceof DaemonApiError ? error.details : undefined
  const code = typeof body === 'object' && body !== null && 'code' in body ? body.code : undefined
  if (code === 'device_group_choices_unlock_required') return 'unlock_required'
  if (code === 'device_group_choices_recovery_required') return 'recovery_required'
  return 'unavailable'
}
