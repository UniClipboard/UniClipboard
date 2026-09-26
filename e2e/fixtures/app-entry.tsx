import { LazyMotion, domMax } from 'framer-motion'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'
import { AppStateFrame } from '@/components/app/AppStateFrame'
import { AppStatusScreen } from '@/components/app/AppStatusScreen'
import { StartupProgressScreen } from '@/components/app/StartupProgressScreen'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import { TitleBar } from '@/components/TitleBar'
import { makeUpgradePreview } from '@/dev/upgrade-preview-model'
import i18n from '@/i18n'
import { pendingStartupSnapshot } from '@/lib/startup-progress'
import ProfileRecoveryPage from '@/pages/ProfileRecoveryPage'
import {
  EntryScreen,
  ImportConfigScreen,
  InitializeSpaceScreen,
  JoinEndedScreen,
  JoinPendingScreen,
  PairingCompleteScreen,
  RedeemInvitationScreen,
  ShowInvitationScreen,
  SpaceReadyScreen,
} from '@/pages/setup/screens'
import UnlockPage from '@/pages/UnlockPage'
import { store } from '@/store'
import './typography.css'

// Real startup, unlock and setup components with synthetic native responses.
// Nothing here reaches a daemon, keyring or the user's profile.
const CORRECT_PASSPHRASE = 'fixture-passphrase'
let callbackId = 0
Object.defineProperty(window, '__TAURI_INTERNALS__', {
  configurable: true,
  value: {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    transformCallback: () => ++callbackId,
    unregisterCallback: () => {},
    invoke: async (command: string, args?: { request?: { passphrase?: string } }) => {
      if (command === 'unlock_content_from_keyring') {
        await new Promise(resolve => setTimeout(resolve, 400))
        return false
      }
      if (command === 'unlock_content') {
        await new Promise(resolve => setTimeout(resolve, 200))
        if (args?.request?.passphrase === CORRECT_PASSPHRASE) {
          document.body.dataset.unlocked = 'true'
          return null
        }
        throw { code: 'WRONG_PASSPHRASE', message: 'fixture' }
      }
      if (command === 'plugin:window|is_maximized') return false
      if (command.startsWith('plugin:window|') || command === 'set_traffic_light_position')
        return null
      if (command === 'plugin:event|listen') return callbackId
      if (command === 'plugin:event|unlisten') return null
      throw new Error(`No fixture for native command: ${command}`)
    },
  },
})
window.fetch = async () => {
  throw new Error('Network access is disabled in the entry fixture')
}

const params = new URLSearchParams(location.search)
await i18n.changeLanguage(params.get('language') || 'zh-CN')
document.documentElement.classList.toggle('dark', params.get('theme') === 'dark')
const noop = () => {}
const expiry = Date.now() + 234_000

function EntryFixture() {
  const view = params.get('view') || 'startup'
  const [setupStep, setSetupStep] = useState(view)
  let content
  if (view === 'startup')
    content = (
      <StartupProgressScreen snapshot={pendingStartupSnapshot} onRetry={noop} onExport={noop} />
    )
  else if (view === 'upgrade')
    content = (
      <StartupProgressScreen
        snapshot={makeUpgradePreview('upgrading', 86)}
        onRetry={noop}
        onExport={noop}
      />
    )
  else if (view === 'upgrade-failed')
    content = (
      <StartupProgressScreen
        snapshot={makeUpgradePreview('backup-failed', 40)}
        onRetry={noop}
        onExport={noop}
      />
    )
  else if (view === 'membership')
    content = (
      <StartupProgressScreen
        snapshot={pendingStartupSnapshot}
        phase="membershipRecovery"
        onRetry={noop}
        onExport={noop}
      />
    )
  else if (view === 'failure')
    content = (
      <AppStatusScreen
        detail={'daemon health check timed out after 10s\nconnection refused: 127.0.0.1:42715'}
        onRetry={noop}
      />
    )
  else if (view === 'version-too-old')
    content = (
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
  else if (view === 'recovery')
    content = (
      <ProfileRecoveryPage
        status={{
          state: 'awaiting_passphrase',
          canSubmitPassphrase: true,
          restartRequired: false,
          backgroundReady: false,
          cleanupPending: false,
          losses: [],
        }}
        onRecovered={noop}
        onRestart={noop}
      />
    )
  else if (view === 'unlock') content = <UnlockPage />
  else if (setupStep === 'setup-entry')
    content = (
      <EntryScreen
        onCreate={() => setSetupStep('setup-initialize')}
        onJoin={() => setSetupStep('setup-redeem')}
        onImport={() => setSetupStep('setup-import')}
      />
    )
  else if (setupStep === 'setup-initialize')
    content = (
      <InitializeSpaceScreen
        onSubmit={async () => ({ ok: false, kind: 'service_unavailable', raw: 'fixture' })}
        onBack={() => setSetupStep('setup-entry')}
      />
    )
  else if (setupStep === 'setup-redeem')
    content = (
      <RedeemInvitationScreen
        onSubmit={async () => ({ ok: false, kind: 'sponsor_unreachable', raw: 'fixture' })}
        onBack={() => setSetupStep('setup-entry')}
      />
    )
  else if (setupStep === 'setup-invite')
    content = <ShowInvitationScreen code="482913" expiresAtMs={expiry} onCancel={noop} />
  else if (setupStep === 'setup-pending')
    content = <JoinPendingScreen status="pending" onCancel={noop} />
  else if (setupStep === 'setup-processing')
    content = <JoinPendingScreen status="processing" onCancel={noop} />
  else if (setupStep === 'setup-ended')
    content = <JoinEndedScreen reason="authentication_rejected" onBack={noop} />
  else if (setupStep === 'setup-ready')
    content = (
      <SpaceReadyScreen
        onInvite={async () => ({ ok: false, kind: 'directory_rejected', raw: 'fixture' })}
        onDone={noop}
      />
    )
  else if (setupStep === 'setup-paired')
    content = (
      <PairingCompleteScreen
        localDeviceName="A deliberately long local device name for layout checks"
        peerDeviceId="12D3KooWfixturepeer9Qa"
        onDone={noop}
      />
    )
  else content = <ImportConfigScreen onBack={() => setSetupStep('setup-entry')} />

  return (
    <div data-testid="entry-surface" className="h-screen">
      <AppStateFrame titleBar={<TitleBar />}>{content}</AppStateFrame>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <Provider store={store}>
    <LazyMotion features={domMax} strict>
      <VisualEffectsProvider>
        <MemoryRouter>
          <EntryFixture />
        </MemoryRouter>
      </VisualEffectsProvider>
    </LazyMotion>
  </Provider>
)
