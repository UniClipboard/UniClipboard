import type { EncryptionStatusView } from '@/lib/daemon-lifecycle-ready'
import type { SetupFlow } from '@/store/setupRealtimeStore'

export function isSetupGateActive(flow: SetupFlow, hydrated: boolean): boolean {
  return !hydrated || flow.kind !== 'completed' || flow.completion !== null
}

export function resolveEncryptionStatus(
  query: EncryptionStatusView | undefined,
  override: EncryptionStatusView | null
): EncryptionStatusView | null {
  return override ?? query ?? null
}
