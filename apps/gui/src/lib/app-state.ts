import type { EncryptionStatusView } from '@/lib/daemon-lifecycle-ready'
import type { SetupFlow } from '@/store/setupRealtimeStore'

export type SetupGate = 'loading' | 'setup' | 'ready'

export function resolveSetupGate(flow: SetupFlow, hydrated: boolean): SetupGate {
  if (!hydrated || flow.kind === 'loading') return 'loading'
  return flow.kind === 'completed' && flow.completion === null ? 'ready' : 'setup'
}

export function resolveEncryptionStatus(
  query: EncryptionStatusView | undefined,
  override: EncryptionStatusView | null
): EncryptionStatusView | null {
  return override ?? query ?? null
}
