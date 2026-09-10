import type { ReactNode } from 'react'
import { exportStartupLogs } from '@/api/startup-support'
import { Toaster } from '@/components/ui/toaster'
import { useAppBootstrap } from '@/hooks/useAppBootstrap'
import { useVisualEffectsSampling } from '@/hooks/useVisualEffectsSampling'
import { startupViewSnapshot } from '@/lib/startup-progress'
import SetupPage from '@/pages/SetupPage'
import UnlockPage from '@/pages/UnlockPage'
import { AppStatusScreen } from './AppStatusScreen'
import { AuthenticatedRoutes } from './AuthenticatedRoutes'
import { UpgradeProgressScreen } from './UpgradeProgressScreen'

type AppContentProps = {
  fullTitleBar: ReactNode
  isSetupActive: boolean
  onSetupComplete: () => void
  sidebarTitle: ReactNode
}

export function AppContent({
  fullTitleBar,
  isSetupActive,
  onSetupComplete,
  sidebarTitle,
}: AppContentProps) {
  const bootstrap = useAppBootstrap(isSetupActive)
  useVisualEffectsSampling(
    !isSetupActive &&
      bootstrap.daemonBootstrapReady &&
      !bootstrap.encryptionLoading &&
      Boolean(bootstrap.resolvedEncryptionStatus?.session_ready)
  )

  if (
    bootstrap.startupStatus &&
    !bootstrap.daemonBootstrapReady &&
    bootstrap.bootstrapFailure?.kind !== 'versionTooOld'
  ) {
    return (
      <div className="flex h-full w-full flex-col">
        {fullTitleBar}
        <UpgradeProgressScreen
          snapshot={startupViewSnapshot(bootstrap.startupStatus, bootstrap.retrying)}
          onRetry={bootstrap.retry}
          onExport={async () => {
            return (await exportStartupLogs()) !== null
          }}
        />
      </div>
    )
  }

  if (bootstrap.bootstrapFailure || bootstrap.retrying) {
    return (
      <div className="flex h-full w-full flex-col">
        {fullTitleBar}
        <AppStatusScreen
          detail={bootstrap.bootEncryptionError ?? bootstrap.bootstrapFailure?.detail}
          failure={bootstrap.bootstrapFailure}
          onRetry={bootstrap.retry}
          retrying={bootstrap.retrying}
        />
      </div>
    )
  }

  if (isSetupActive) {
    return (
      <>
        <SetupPage onCompleteSetup={onSetupComplete} />
        <Toaster />
      </>
    )
  }

  if (
    bootstrap.encryptionLoading &&
    bootstrap.encryptionOverride === null &&
    !bootstrap.encryptionError
  ) {
    return null
  }

  if (bootstrap.encryptionError) {
    return (
      <div className="flex h-full w-full flex-col">
        {fullTitleBar}
        <AppStatusScreen detail={bootstrap.encryptionError} onRetry={bootstrap.retry} />
      </div>
    )
  }

  if (!bootstrap.daemonBootstrapReady && bootstrap.encryptionOverride === null) return null

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
