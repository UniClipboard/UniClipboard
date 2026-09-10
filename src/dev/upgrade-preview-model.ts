import type { StartupSnapshot, StepProgress } from '@/lib/startup-progress'

export const previewScenarios = [
  'upgrading',
  'cold-start',
  'recovering',
  'unknown',
  'finishing',
  'verifying',
  'failed',
  'protection',
  'interrupted',
  'starting',
  'ready',
] as const
export type PreviewScenario = (typeof previewScenarios)[number]

export function makeUpgradePreview(
  scenario: PreviewScenario,
  seconds: number,
  upgradeRequired = scenario !== 'cold-start'
): StartupSnapshot {
  const finished = scenario === 'starting' || scenario === 'ready'
  const current: StepProgress = {
    step: scenario === 'verifying' ? 'verifying' : 'converting_contents',
    processed:
      scenario === 'verifying'
        ? 0
        : scenario === 'finishing'
          ? 4098
          : Math.min(4098, Math.floor((seconds / 40) * 4098)),
    total: scenario === 'unknown' || scenario === 'verifying' ? null : 4098,
    unit: scenario === 'verifying' ? null : 'content_representations',
    warning_count: scenario === 'recovering' ? null : 0,
    completed: finished,
  }
  const failure =
    scenario === 'failed'
      ? { reason: 'storage_full' as const, retryable: true }
      : scenario === 'protection'
        ? { reason: 'protection_unavailable' as const, retryable: false }
        : null
  const state = failure
    ? 'failed'
    : scenario === 'interrupted'
      ? 'interrupted'
      : scenario === 'ready'
        ? 'ready'
        : scenario === 'starting' || scenario === 'cold-start'
          ? 'starting_services'
          : 'upgrading'
  return {
    attempt_id: 'development-preview',
    sequence: Math.floor(seconds * 4),
    state,
    elapsed_ms: seconds * 1000,
    upgrade: {
      required: upgradeRequired,
      recovering: scenario === 'recovering',
      completed: finished,
      current_step: finished ? null : current.step,
      steps: [
        {
          step: 'checking',
          processed: 0,
          total: null,
          unit: null,
          warning_count: 0,
          completed: true,
        },
        { ...current, processed: finished ? 4098 : current.processed },
        ...(finished
          ? [
              {
                step: 'verifying' as const,
                processed: 0,
                total: null,
                unit: null,
                warning_count: 0,
                completed: true,
              },
            ]
          : []),
      ],
    },
    failure,
    allowed_actions: {
      retry: failure?.retryable === true || state === 'interrupted',
      export_diagnostics: true,
    },
  }
}
