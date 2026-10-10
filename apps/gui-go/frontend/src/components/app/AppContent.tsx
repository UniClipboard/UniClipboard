import { type ReactNode } from 'react'
import { useAppBootstrap } from '@/hooks/useAppBootstrap'
import { useContentUnlocked } from '@/hooks/useContentUnlocked'
import { useMainWindowPresentation } from '@/hooks/useMainWindowPresentation'
import { useProfileRecovery } from '@/hooks/useProfileRecovery'
import { useSettledAppView } from '@/hooks/useSettledAppView'
import { useVisualEffectsSampling } from '@/hooks/useVisualEffectsSampling'
import type { SetupGate } from '@/lib/app-state'
import { resolveAppContentState } from './app-content-state'
import { AppContentView } from './AppContentView'

type AppContentProps = {
  fullTitleBar: ReactNode
  setupGate: SetupGate
  onSetupComplete: () => void
}

export function AppContent({ fullTitleBar, setupGate, onSetupComplete }: AppContentProps) {
  const bootstrap = useAppBootstrap(setupGate !== 'ready')
  const recovery = useProfileRecovery(bootstrap.daemonBootstrapReady)
  const contentLock = useContentUnlocked(
    setupGate === 'ready' &&
      bootstrap.daemonBootstrapReady &&
      recovery.status?.backgroundReady === true
  )
  const contentUnlocked = contentLock.unlocked
  useVisualEffectsSampling(
    setupGate === 'ready' &&
      bootstrap.daemonBootstrapReady &&
      !bootstrap.encryptionLoading &&
      Boolean(bootstrap.resolvedEncryptionStatus?.session_ready) &&
      bootstrap.spaceReadiness === 'ready'
  )

  const appState = resolveAppContentState({
    backgroundReady: recovery.status?.backgroundReady ?? null,
    recoveryFailed: recovery.failed,
    startupStatus: bootstrap.startupStatus,
    daemonBootstrapReady: bootstrap.daemonBootstrapReady,
    retrying: bootstrap.retrying,
    bootstrapFailure: Boolean(bootstrap.bootstrapFailure),
    versionTooOld: bootstrap.bootstrapFailure?.kind === 'versionTooOld',
    encryptionError: Boolean(bootstrap.encryptionError),
    setupGate,
    hasEncryptionStatus: Boolean(bootstrap.resolvedEncryptionStatus),
    sessionReady: bootstrap.resolvedEncryptionStatus?.session_ready === true,
    encryptionInitialized: bootstrap.resolvedEncryptionStatus?.initialized === true,
    contentUnlocked,
    spaceReadiness: bootstrap.spaceReadiness,
  })
  useMainWindowPresentation(
    (recovery.status !== null && !recovery.status.backgroundReady) ||
      appState.showFailure ||
      !appState.showStartup ||
      appState.needsAttention ||
      bootstrap.spaceReadiness === 'recoveringMembership'
  )
  const view = useSettledAppView(appState.view)
  return (
    <AppContentView
      bootstrap={bootstrap}
      contentLock={contentLock}
      fullTitleBar={fullTitleBar}
      hasStartupTask={appState.hasStartupTask}
      onSetupComplete={onSetupComplete}
      recovery={recovery}
      view={view}
    />
  )
}
