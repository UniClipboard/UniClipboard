import type {
  DaemonStartupStatus,
  StartupSnapshotDto,
  StartupStepProgressDto,
} from '@/lib/ipc-bindings.generated'

export type StartupSnapshot = StartupSnapshotDto
export type StepProgress = StartupStepProgressDto

export function startupPresentation(snapshot: StartupSnapshot) {
  const required = snapshot.upgrade?.required === true
  const failed = snapshot.state === 'failed' || snapshot.state === 'interrupted'
  const ready = snapshot.state === 'ready'
  let title = 'preparing'
  if (failed) title = required ? 'failed' : 'startupFailed'
  else if (ready) title = required ? 'ready' : 'startupReady'
  else if (required && snapshot.state === 'starting_services') title = 'starting'
  else if (required && snapshot.state === 'upgrading')
    title = snapshot.upgrade?.recovering ? 'recovering' : 'title'
  return {
    required,
    failed,
    ready,
    title,
    showProgress: required && !failed && !ready,
    showActivity: required && Boolean(snapshot.upgrade?.steps.length),
  }
}

export function stepPercentage(step: StepProgress | undefined): number | null {
  if (
    !step ||
    step.total === null ||
    step.total <= 0 ||
    step.completed ||
    step.processed >= step.total
  )
    return null
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
