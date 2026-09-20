import { startupPresentation, stepPercentage, type StartupSnapshot } from '@/lib/startup-progress'

export function resolveStartupScreenState(
  snapshot: StartupSnapshot,
  phase: 'default' | 'membershipRecovery'
) {
  const current = snapshot.upgrade?.steps.find(step => step.step === snapshot.upgrade?.current_step)
  const presentation = startupPresentation(snapshot)
  const { required, failed, ready } = presentation
  const icon = failed ? 'failed' : ready ? 'ready' : 'working'
  const categoryTone = failed ? 'destructive' : ready ? 'success' : 'default'
  const title = phase === 'membershipRecovery' ? 'membershipRecovering' : presentation.title
  let description = 'startingDescription'
  if (phase === 'membershipRecovery') description = 'membershipRecoveringDescription'
  else if (failed) description = `errors.${snapshot.failure?.reason ?? 'interrupted'}`
  else if (ready && required) description = 'readyDescription'
  else if (required && snapshot.state === 'upgrading') description = 'description'

  return {
    ...presentation,
    category: required ? 'category' : 'startupCategory',
    categoryTone,
    current,
    description,
    finishingStep: Boolean(
      current &&
      current.unit !== null &&
      current.total !== null &&
      current.processed >= current.total
    ),
    icon,
    percentage: stepPercentage(current),
    showElapsed: !required && !failed && !ready,
    title,
  } as const
}
