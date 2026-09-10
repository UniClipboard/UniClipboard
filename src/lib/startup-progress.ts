import type {
  DaemonStartupStatus,
  StartupSnapshotDto,
  StartupStepProgressDto,
} from '@/lib/ipc-bindings.generated'

export type StartupSnapshot = StartupSnapshotDto
export type StepProgress = StartupStepProgressDto

export function stepPercentage(step: StepProgress | undefined): number | null {
  if (!step || step.total === null || step.total <= 0) return null
  return Math.min(100, Math.max(0, Math.floor((step.processed / step.total) * 100)))
}

// The host's service readiness is distinct from Engine's final state.
export function startupViewSnapshot(
  status: DaemonStartupStatus,
  retrying: boolean
): StartupSnapshot {
  const progress = status.progress
  if (retrying && (status.service_failed || ['failed', 'interrupted'].includes(progress.state))) {
    return {
      ...progress,
      state: 'preparing',
      failure: null,
      allowed_actions: { retry: false, export_diagnostics: true },
    }
  }
  if (status.service_failed) {
    return {
      ...progress,
      state: 'failed',
      failure: { reason: 'startup_failed', retryable: true },
      allowed_actions: { retry: true, export_diagnostics: true },
    }
  }
  if (progress.state === 'ready') return { ...progress, state: 'starting_services' }
  return progress
}
