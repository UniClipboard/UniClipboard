import type { RelayProbeOutcome } from '@/api/daemon/settings'
export type ProbeStatus =
  | { kind: 'idle' }
  | { kind: 'testing'; pendingUrl: string }
  | { kind: 'success'; latencyMs: number }
  | { kind: 'failure'; message: string }

export function canonicalRelayUrl(value: string): string | null {
  try {
    return new URL(value.trim()).toString()
  } catch {
    return null
  }
}

export function outcomeToStatus(
  outcome: RelayProbeOutcome,
  t: (key: string, options?: Record<string, unknown>) => string
): ProbeStatus {
  switch (outcome.kind) {
    case 'success':
      return { kind: 'success', latencyMs: outcome.latencyMs }
    case 'invalidUrl':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.invalidUrl', {
          message: outcome.message,
        }),
      }
    case 'dns':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.dns', {
          message: outcome.message,
        }),
      }
    case 'tls':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.tls', {
          message: outcome.message,
        }),
      }
    case 'handshake':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.handshake', {
          message: outcome.message,
        }),
      }
    case 'timeout':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.timeout'),
      }
    case 'other':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.other', {
          message: outcome.message,
        }),
      }
  }
}
