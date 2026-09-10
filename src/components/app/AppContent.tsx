import type { ReactNode } from 'react'
import { exportStartupLogs } from '@/api/startup-support'
import { Toaster } from '@/components/ui/toaster'
import { useAppBootstrap } from '@/hooks/useAppBootstrap'
import { useMainWindowPresentation } from '@/hooks/useMainWindowPresentation'
import { useVisualEffectsSampling } from '@/hooks/useVisualEffectsSampling'
import type { SetupGate } from '@/lib/app-state'
import { startupFailed } from '@/lib/daemon-startup-progress'
import { pendingStartupSnapshot, startupViewSnapshot } from '@/lib/startup-progress'
import SetupPage from '@/pages/SetupPage'
import UnlockPage from '@/pages/UnlockPage'
import { AppStatusScreen } from './AppStatusScreen'
import { AuthenticatedRoutes } from './AuthenticatedRoutes'
import { StartupProgressScreen } from './StartupProgressScreen'

type AppContentProps = {
  fullTitleBar: ReactNode
  setupGate: SetupGate
  onSetupComplete: () => void
  sidebarTitle: ReactNode
}

export function AppContent({
  fullTitleBar,
  setupGate,
  onSetupComplete,
  sidebarTitle,
}: AppContentProps) {
  const bootstrap = useAppBootstrap(setupGate !== 'ready')
  useVisualEffectsSampling(
    setupGate === 'ready' &&
      bootstrap.daemonBootstrapReady &&
      !bootstrap.encryptionLoading &&
      Boolean(bootstrap.resolvedEncryptionStatus?.session_ready)
  )

  const hasStartupTask = Boolean(bootstrap.startupStatus && !bootstrap.daemonBootstrapReady)
  const showFailure =
    bootstrap.bootstrapFailure?.kind === 'versionTooOld' ||
    (!hasStartupTask &&
      !bootstrap.retrying &&
      Boolean(bootstrap.bootstrapFailure || bootstrap.encryptionError))
  const showStartup =
    hasStartupTask ||
    bootstrap.retrying ||
    !bootstrap.daemonBootstrapReady ||
    setupGate === 'loading' ||
    (setupGate === 'ready' && !bootstrap.resolvedEncryptionStatus)
  const needsAttention = Boolean(
    hasStartupTask &&
    bootstrap.startupStatus &&
    (startupFailed(bootstrap.startupStatus) ||
      (!bootstrap.startupStatus.service_ready &&
        bootstrap.startupStatus.progress.state === 'upgrading' &&
        bootstrap.startupStatus.progress.upgrade?.required))
  )
  useMainWindowPresentation(showFailure || !showStartup || needsAttention)
  if (showFailure) {
    return (
      <div className="flex h-full w-full flex-col">
        {fullTitleBar}
        <AppStatusScreen
          detail={
            bootstrap.encryptionError ??
            bootstrap.bootEncryptionError ??
            bootstrap.bootstrapFailure?.detail
          }
          failure={bootstrap.bootstrapFailure}
          onRetry={bootstrap.retry}
        />
      </div>
    )
  }

  if (showStartup) {
    return (
      <div className="flex h-full w-full flex-col">
        {fullTitleBar}
        <StartupProgressScreen
          snapshot={
            hasStartupTask && bootstrap.startupStatus
              ? startupViewSnapshot(bootstrap.startupStatus, bootstrap.retrying)
              : pendingStartupSnapshot
          }
          onRetry={bootstrap.retry}
          onExport={async () => (await exportStartupLogs()) !== null}
        />
      </div>
    )
  }

  if (setupGate === 'setup') {
    return (
      <>
        <SetupPage onCompleteSetup={onSetupComplete} />
        <Toaster />
      </>
    )
  }

  if (
    bootstrap.resolvedEncryptionStatus?.initialized &&
    !bootstrap.resolvedEncryptionStatus.session_ready
  ) {
    return (
      <div className="flex h-full w-full flex-col">
        {fullTitleBar}
        <div className="min-h-0 flex-1">
          <UnlockPage
            onUnlockSucceeded={() =>
              bootstrap.setEncryptionStatus({ initialized: true, session_ready: true })
            }
            onResetSucceeded={() =>
              bootstrap.setEncryptionStatus({ initialized: false, session_ready: false })
            }
          />
        </div>
      </div>
    )
  }

  return <AuthenticatedRoutes fullTitleBar={fullTitleBar} sidebarTitle={sidebarTitle} />
}
