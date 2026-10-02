import type { SetupGate } from '@/lib/app-state'
import { startupFailed } from '@/lib/daemon-startup-progress'
import type { DaemonStartupStatus } from '@/lib/ipc'

export type AppContentView =
  | 'profile-recovery'
  | 'recovery-query-failed'
  | 'failure'
  | 'upgrade'
  | 'startup'
  | 'setup'
  | 'unlock'
  | 'authenticated'

type AppContentStateInput = {
  backgroundReady: boolean | null
  recoveryFailed: boolean
  startupStatus: DaemonStartupStatus | null
  daemonBootstrapReady: boolean
  retrying: boolean
  bootstrapFailure: boolean
  versionTooOld: boolean
  encryptionError: boolean
  setupGate: SetupGate
  hasEncryptionStatus: boolean
  sessionReady: boolean
  encryptionInitialized: boolean
  contentUnlocked: boolean | null
  spaceReadiness: 'checking' | 'recoveringMembership' | 'ready'
}

export function resolveAppContentState(input: AppContentStateInput) {
  const hasStartupTask = Boolean(input.startupStatus && !input.daemonBootstrapReady)
  const showFailure =
    input.versionTooOld ||
    (!hasStartupTask && !input.retrying && (input.bootstrapFailure || input.encryptionError))
  const showStartup =
    input.backgroundReady === null ||
    hasStartupTask ||
    input.retrying ||
    !input.daemonBootstrapReady ||
    input.setupGate === 'loading' ||
    (input.setupGate === 'ready' && !input.hasEncryptionStatus) ||
    (input.setupGate === 'ready' && input.contentUnlocked === null) ||
    (input.setupGate === 'ready' && input.sessionReady && input.spaceReadiness === 'checking')
  const needsAttention = Boolean(
    hasStartupTask &&
    input.startupStatus &&
    (startupFailed(input.startupStatus) ||
      (!input.startupStatus.service_ready &&
        input.startupStatus.progress.state === 'upgrading' &&
        input.startupStatus.progress.upgrade?.required))
  )

  let view: AppContentView
  if (input.backgroundReady === false) view = 'profile-recovery'
  else if (input.recoveryFailed) view = 'recovery-query-failed'
  else if (showFailure) view = 'failure'
  else if (showStartup)
    view = input.startupStatus?.progress.upgrade?.required ? 'upgrade' : 'startup'
  else if (input.setupGate === 'setup') view = 'setup'
  else if (input.encryptionInitialized && (!input.contentUnlocked || !input.sessionReady))
    view = 'unlock'
  else view = 'authenticated'

  return { hasStartupTask, needsAttention, showFailure, showStartup, view }
}
