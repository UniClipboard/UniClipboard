import type { ReactNode } from 'react'
import { exportStartupLogs } from '@/api/startup-support'
import { Toaster } from '@/components/ui/toaster'
import type { useAppBootstrap } from '@/hooks/useAppBootstrap'
import type { useContentUnlocked } from '@/hooks/useContentUnlocked'
import type { useProfileRecovery } from '@/hooks/useProfileRecovery'
import { pendingStartupSnapshot, startupViewSnapshot } from '@/lib/startup-progress'
import ProfileRecoveryPage from '@/pages/ProfileRecoveryPage'
import SetupPage from '@/pages/SetupPage'
import UnlockPage from '@/pages/UnlockPage'
import type { AppContentView as View } from './app-content-state'
import { AppStateFrame } from './AppStateFrame'
import { AppStatusScreen } from './AppStatusScreen'
import { AppViewTransition } from './AppViewTransition'
import { AuthenticatedRoutes } from './AuthenticatedRoutes'
import { StartupProgressScreen } from './StartupProgressScreen'

type Props = {
  bootstrap: ReturnType<typeof useAppBootstrap>
  contentLock: ReturnType<typeof useContentUnlocked>
  fullTitleBar: ReactNode
  hasStartupTask: boolean
  onSetupComplete: () => void
  recovery: ReturnType<typeof useProfileRecovery>
  sidebarTitle: ReactNode
  view: View
}

export function AppContentView({
  bootstrap,
  contentLock,
  fullTitleBar,
  hasStartupTask,
  onSetupComplete,
  recovery,
  sidebarTitle,
  view,
}: Props) {
  let content: ReactNode
  if (view === 'profile-recovery' && recovery.status) {
    content = (
      <AppStateFrame titleBar={fullTitleBar}>
        <ProfileRecoveryPage
          status={recovery.status}
          onRestart={bootstrap.retry}
          onRecovered={() => {
            recovery.refresh()
            contentLock.refresh()
          }}
        />
      </AppStateFrame>
    )
  } else if (view === 'recovery-query-failed') {
    content = (
      <AppStateFrame titleBar={fullTitleBar}>
        <AppStatusScreen detail={null} onRetry={recovery.refresh} />
      </AppStateFrame>
    )
  } else if (view === 'failure') {
    content = (
      <AppStateFrame titleBar={fullTitleBar}>
        <AppStatusScreen
          detail={
            bootstrap.encryptionError ??
            bootstrap.bootEncryptionError ??
            bootstrap.bootstrapFailure?.detail
          }
          failure={bootstrap.bootstrapFailure}
          onRetry={bootstrap.retry}
        />
      </AppStateFrame>
    )
  } else if (view === 'upgrade' || view === 'startup') {
    const snapshot =
      hasStartupTask && bootstrap.startupStatus
        ? startupViewSnapshot(bootstrap.startupStatus, bootstrap.retrying)
        : pendingStartupSnapshot
    content = (
      <AppStateFrame titleBar={fullTitleBar}>
        <StartupProgressScreen
          phase={
            bootstrap.spaceReadiness === 'recoveringMembership' ? 'membershipRecovery' : 'default'
          }
          snapshot={snapshot}
          onRetry={bootstrap.retry}
          onExport={async () => (await exportStartupLogs()) !== null}
        />
      </AppStateFrame>
    )
  } else if (view === 'setup') {
    content = (
      <AppStateFrame titleBar={fullTitleBar}>
        <SetupPage
          onCompleteSetup={() => {
            bootstrap.setEncryptionStatus({ initialized: true, session_ready: true })
            onSetupComplete()
          }}
        />
        <Toaster />
      </AppStateFrame>
    )
  } else if (view === 'unlock') {
    content = (
      <AppStateFrame titleBar={fullTitleBar}>
        <UnlockPage
          onUnlockSucceeded={() => {
            bootstrap.setEncryptionStatus({ initialized: true, session_ready: true })
            contentLock.refresh()
          }}
          onResetSucceeded={() =>
            bootstrap.setEncryptionStatus({ initialized: false, session_ready: false })
          }
        />
      </AppStateFrame>
    )
  } else {
    content = <AuthenticatedRoutes fullTitleBar={fullTitleBar} sidebarTitle={sidebarTitle} />
  }

  return <AppViewTransition viewKey={view}>{content}</AppViewTransition>
}
