// The startup, recovery, unlock and setup pages of the platform UI kit with synthetic native
// responses. Build it with a target, like the gallery:
//   UC_UI_PLATFORM=macos UI_FIXTURE_ENTRY=e2e/fixtures/app-entry-kit.tsx node e2e/visual-effects-server.mjs
// `?view=…&theme=light|dark&language=…`. Nothing here reaches a daemon, keyring or the user's profile.
import { LazyMotion, domMax } from 'framer-motion'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'
import { AppStateFrame } from '@/components/app/AppStateFrame'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import { TitleBar } from '@/components/TitleBar'
import { makeUpgradePreview } from '@/dev/upgrade-preview-model'
import i18n from '@/i18n'
import { pendingStartupSnapshot } from '@/lib/startup-progress'
import ProfileRecoveryPage from '@/pages/entry-kit/ProfileRecoveryKitPage'
import { SetupKitEntry } from '@/pages/entry-kit/SetupKitEntry'
import { SetupKitImport } from '@/pages/entry-kit/SetupKitImport'
import { SetupKitInitialize } from '@/pages/entry-kit/SetupKitInitialize'
import { SetupKitInvitation } from '@/pages/entry-kit/SetupKitInvitation'
import { SetupKitJoinEnded } from '@/pages/entry-kit/SetupKitJoinEnded'
import { SetupKitJoinPending } from '@/pages/entry-kit/SetupKitJoinPending'
import { SetupKitPaired } from '@/pages/entry-kit/SetupKitPaired'
import { SetupKitRedeem } from '@/pages/entry-kit/SetupKitRedeem'
import { SetupKitSpaceReady } from '@/pages/entry-kit/SetupKitSpaceReady'
import { StartupProgressScreen } from '@/pages/entry-kit/StartupKitScreen'
import { AppStatusScreen } from '@/pages/entry-kit/StatusKitScreen'
import UnlockPage from '@/pages/entry-kit/UnlockKitPage'
import { store } from '@/store'
import { HostRejection, installFakeHost } from './fake-host'
import '@/App.css'

const CORRECT_PASSPHRASE = 'fixture-passphrase'
installFakeHost({
  UnlockContentFromKeyring: async () => {
    await new Promise(resolve => setTimeout(resolve, 400))
    return false
  },
  UnlockContent: async (request: unknown) => {
    await new Promise(resolve => setTimeout(resolve, 200))
    if ((request as { passphrase?: string })?.passphrase === CORRECT_PASSPHRASE) {
      document.body.dataset.unlocked = 'true'
      return null
    }
    throw new HostRejection({ code: 'WRONG_PASSPHRASE', message: 'fixture' })
  },
})
window.fetch = async () => {
  throw new Error('Network access is disabled in the entry fixture')
}

const params = new URLSearchParams(location.search)
await i18n.changeLanguage(params.get('language') || 'zh-CN')
document.documentElement.classList.toggle('dark', params.get('theme') === 'dark')
const noop = () => {}
const recoveryStatus = {
  state: 'awaiting_passphrase' as const,
  canSubmitPassphrase: true,
  restartRequired: false,
  backgroundReady: false,
  cleanupPending: false,
  losses: [],
}

function View({ view }: { view: string }) {
  const [setupStep, setSetupStep] = useState(view)
  const expiry = view === 'setup-invite-expired' ? Date.now() - 1000 : Date.now() + 234_000
  if (view === 'startup')
    return (
      <StartupProgressScreen snapshot={pendingStartupSnapshot} onRetry={noop} onExport={noop} />
    )
  if (view === 'upgrade')
    return (
      <StartupProgressScreen
        snapshot={makeUpgradePreview('upgrading', 18)}
        onRetry={noop}
        onExport={noop}
      />
    )
  if (view === 'upgrade-failed')
    return (
      <StartupProgressScreen
        snapshot={makeUpgradePreview('backup-failed', 40)}
        onRetry={noop}
        onExport={noop}
      />
    )
  if (view === 'membership')
    return (
      <StartupProgressScreen
        snapshot={pendingStartupSnapshot}
        phase="membershipRecovery"
        onRetry={noop}
        onExport={noop}
      />
    )
  if (view === 'failure')
    return (
      <AppStatusScreen
        detail={'daemon health check timed out after 10s\nconnection refused: 127.0.0.1:42715'}
        onRetry={noop}
      />
    )
  if (view === 'version-too-old')
    return (
      <AppStatusScreen
        detail={null}
        failure={{
          kind: 'versionTooOld',
          detail: 'fixture',
          observedVersion: '1.1.0',
          expectedVersion: '1.0.0',
        }}
        onRetry={noop}
      />
    )
  if (view === 'recovery')
    return <ProfileRecoveryPage status={recoveryStatus} onRecovered={noop} onRestart={noop} />
  if (view === 'recovery-restart')
    return (
      <ProfileRecoveryPage
        status={{ ...recoveryStatus, restartRequired: true, canSubmitPassphrase: false }}
        onRecovered={noop}
        onRestart={noop}
      />
    )
  if (view === 'admission')
    return (
      <ProfileRecoveryPage
        status={{
          ...recoveryStatus,
          state: 'admission_recovery_required',
          canSubmitPassphrase: false,
          admission: {
            category: 'credential_missing',
            stage: 'credential',
            action: 'restore_credential',
          },
        }}
        onRecovered={noop}
        onRestart={noop}
      />
    )
  if (view === 'unlock') return <UnlockPage />
  if (setupStep === 'setup-entry')
    return (
      <SetupKitEntry
        onCreate={() => setSetupStep('setup-initialize')}
        onJoin={() => setSetupStep('setup-redeem')}
        onImport={() => setSetupStep('setup-import')}
      />
    )
  if (setupStep === 'setup-initialize')
    return (
      <SetupKitInitialize
        onSubmit={async () => ({ ok: false, kind: 'service_unavailable', raw: 'fixture' })}
        onBack={() => setSetupStep('setup-entry')}
      />
    )
  if (setupStep === 'setup-redeem')
    return (
      <SetupKitRedeem
        onSubmit={async () => ({ ok: false, kind: 'sponsor_unreachable', raw: 'fixture' })}
        onBack={() => setSetupStep('setup-entry')}
      />
    )
  if (setupStep === 'setup-invite' || setupStep === 'setup-invite-expired')
    return <SetupKitInvitation code="482913" expiresAtMs={expiry} onCancel={noop} />
  if (setupStep === 'setup-pending') return <SetupKitJoinPending status="pending" onCancel={noop} />
  if (setupStep === 'setup-processing')
    return <SetupKitJoinPending status="processing" onCancel={noop} />
  if (setupStep === 'setup-ended')
    return <SetupKitJoinEnded reason="authentication_rejected" onBack={noop} />
  if (setupStep === 'setup-ready')
    return (
      <SetupKitSpaceReady
        onInvite={async () => ({ ok: false, kind: 'directory_rejected', raw: 'fixture' })}
        onDone={noop}
      />
    )
  if (setupStep === 'setup-paired')
    return (
      <SetupKitPaired
        localDeviceName="A deliberately long local device name for layout checks"
        peerDeviceId="12D3KooWfixturepeer9Qa"
        onDone={noop}
      />
    )
  return <SetupKitImport onBack={() => setSetupStep('setup-entry')} />
}

export default function EntryKitFixture() {
  return (
    <div data-testid="entry-surface" className="flex h-screen flex-col">
      <AppStateFrame titleBar={<TitleBar />}>
        <View view={params.get('view') || 'startup'} />
      </AppStateFrame>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <Provider store={store}>
    <LazyMotion features={domMax} strict>
      <VisualEffectsProvider>
        <MemoryRouter>
          <EntryKitFixture />
        </MemoryRouter>
      </VisualEffectsProvider>
    </LazyMotion>
  </Provider>
)
