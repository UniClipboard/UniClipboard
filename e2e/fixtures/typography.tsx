import { LazyMotion, domMax } from 'framer-motion'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'
import { AppStatusScreen } from '@/components/app/AppStatusScreen'
import { StartupProgressScreen } from '@/components/app/StartupProgressScreen'
import ClipboardPreviewInfo from '@/components/clipboard/ClipboardPreviewInfo'
import CodePreview from '@/components/clipboard/preview-renderers/CodePreview'
import TextPreview from '@/components/clipboard/preview-renderers/TextPreview'
import { AddDeviceInvitation } from '@/components/device/AddDeviceInvitation'
import DeviceListItem from '@/components/device/DeviceListItem'
import LocalDevicePanel from '@/components/device/LocalDevicePanel'
import { FeedbackDialog } from '@/components/feedback/FeedbackDialog'
import HistoryGridRow from '@/components/history/HistoryGridRow'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import { Button } from '@/components/ui/button'
import { ReleaseNotes } from '@/components/update/ReleaseNotes'
import { SettingContext } from '@/contexts/setting-context'
import { makeUpgradePreview } from '@/dev/upgrade-preview-model'
import i18n from '@/i18n'
import type { DisplayClipboardItem, ClipboardTextItem } from '@/lib/clipboard-entry'
import {
  EntryScreen,
  InitializeSpaceScreen,
  ShowInvitationScreen,
  RedeemInvitationScreen,
  JoinPendingScreen,
  JoinRejectedScreen,
  SpaceReadyScreen,
  PairingCompleteScreen,
  ImportConfigScreen,
} from '@/pages/setup/screens'
import UnlockPage from '@/pages/UnlockPage'
import { store } from '@/store'
import { updateTransferProgress } from '@/store/slices/fileTransferSlice'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType } from '@/types/setting'
import { installSettingsFixtureEnvironment } from './settings-environment'
import './typography.css'

installSettingsFixtureEnvironment()
const params = new URLSearchParams(location.search)
await i18n.changeLanguage(params.get('language') || 'zh-CN')
document.documentElement.classList.toggle('dark', params.get('theme') === 'dark')
const noop = () => {}
const resolved = async () => {}
const settings: SettingContextType = {
  setting: makeBaseSettings(),
  loading: false,
  error: null,
  reloadSetting: resolved,
  updateSetting: resolved,
  updateGeneralSetting: resolved,
  updateAutostart: resolved,
  updateSyncSetting: resolved,
  updateSecuritySetting: resolved,
  updateRetentionPolicy: resolved,
  updateFileSyncSetting: resolved,
  updateKeyboardShortcuts: resolved,
  updateNetworkSetting: async () => ({ restartRequired: false }),
  updateQuickPanelSetting: async () => ({ restartRequired: false }),
  saveRelay: async () => ({ restartRequired: false, credentialStatus: { configured: false } }),
}
const sampleText =
  'Typography verification: a long clipboard entry with punctuation, numbers 1234567890 and multiple lines. '.repeat(
    3
  )
const items: DisplayClipboardItem[] = [
  {
    id: 'image',
    type: 'image',
    activeTime: Date.now(),
    content: { width: 1206, height: 2622, size: 2048 },
  },
  {
    id: 'text',
    type: 'text',
    activeTime: Date.now(),
    content: { display_text: sampleText, has_detail: false, size: 320 },
  },
  {
    id: 'code',
    type: 'text',
    contentTags: ['code'],
    activeTime: Date.now(),
    content: {
      display_text: 'const message = "A long code sample";\nconsole.log(message);',
      has_detail: false,
      size: 80,
    },
  },
  {
    id: 'link',
    type: 'text',
    contentTags: ['link'],
    activeTime: Date.now(),
    content: {
      display_text: 'https://example.com/a-very-long-path/typography-verification',
      link_urls: ['https://example.com/a-very-long-path/typography-verification'],
      has_detail: false,
      size: 80,
    },
  },
  {
    id: 'file',
    type: 'file',
    activeTime: Date.now(),
    content: {
      file_names: ['Quarterly report with a long document name.docx'],
      file_sizes: [12345678],
    },
  },
]
const seenIds = new Set(items.map(item => item.id))
const expiry = Date.now() + 300000

if (params.get('transfer') === '1') {
  store.dispatch(
    updateTransferProgress({
      transferId: 'fixture-transfer',
      entryId: 'file',
      peerId: 'fixture-peer',
      direction: 'receiving',
      bytesTransferred: 1024,
      totalBytes: 2048,
    })
  )
}

export default function TypographyFixture() {
  const view = params.get('view') || 'history'
  const [selected, setSelected] = useState('text')
  const [feedback, setFeedback] = useState(view === 'feedback')
  const [step, setStep] = useState(view)
  const [copied, setCopied] = useState<string | null>(null)
  let content
  if (view === 'history') {
    const item = items.find(entry => entry.id === selected)!
    content = (
      <div className="grid h-full min-h-0 grid-cols-1 md:grid-cols-2">
        <div className="min-w-0 overflow-y-auto border-r border-border" data-testid="history-list">
          {items.map(entry => (
            <HistoryGridRow
              key={entry.id}
              item={entry}
              seenIds={seenIds}
              isActive={entry.id === selected}
              copySuccess={entry.id === copied}
              isDeleting={false}
              showDivider
              onCopy={setCopied}
              onFilePathsAction={noop}
              onDelete={noop}
              onToggleFavorite={noop}
              onClick={setSelected}
              onHoverChange={noop}
            />
          ))}
        </div>
        <div className="flex min-h-0 min-w-0 flex-col overflow-auto" data-testid="history-preview">
          {item.id === 'code' ? (
            <CodePreview
              item={{ code: (item.content as ClipboardTextItem).display_text }}
              preview={null}
            />
          ) : (
            <TextPreview
              item={items.find(entry => entry.id === 'text')!.content as ClipboardTextItem}
              loading={false}
              preview={null}
            />
          )}
          <div className="mt-auto p-4">
            <ClipboardPreviewInfo
              item={item}
              preview={null}
              delivery={null}
              imageDimensions={null}
            />
          </div>
        </div>
      </div>
    )
  } else if (view === 'devices') {
    content = (
      <div className="grid h-full min-h-0 grid-cols-1 md:grid-cols-[15rem_1fr]">
        <div className="min-w-0 p-3" data-device-list>
          <DeviceListItem
            name="MacBook Pro with a deliberately long device name"
            tone="success"
            status={{ kind: 'online', label: 'Connected and synchronizing' }}
            selected
            onSelect={noop}
          />
          <DeviceListItem
            name="Windows workstation"
            tone="off"
            status={{ kind: 'offline', label: 'Offline - last seen a few minutes ago' }}
            selected={false}
            onSelect={noop}
          />
        </div>
        <div className="min-w-0 overflow-auto">
          <LocalDevicePanel
            localDevice={{
              peerId: 'fixture-device-123456789',
              deviceName: 'MacBook Pro with a deliberately long device name',
            }}
            memberCount={3}
          />
        </div>
      </div>
    )
  } else if (view === 'unlock') content = <UnlockPage />
  else if (view === 'startup')
    content = (
      <StartupProgressScreen
        snapshot={makeUpgradePreview('upgrading', 20)}
        onRetry={noop}
        onExport={noop}
      />
    )
  else if (view === 'failure')
    content = (
      <AppStatusScreen
        detail="The local service could not be started. Please try again."
        onRetry={noop}
      />
    )
  else if (view === 'feedback')
    content = (
      <>
        <Button onClick={() => setFeedback(true)}>Feedback</Button>
        <FeedbackDialog open={feedback} onOpenChange={setFeedback} />
      </>
    )
  else if (view === 'release')
    content = (
      <div className="p-6">
        <ReleaseNotes
          content={
            '## Changes\n\nA readable description with **emphasis**, `code` and [a link](https://example.com).\n\n- First change\n- Second change'
          }
          fallback="No changes"
        />
      </div>
    )
  else if (view === 'invitation')
    content = (
      <div className="mx-auto w-full max-w-lg p-6">
        <AddDeviceInvitation
          invitation={{ code: '123456789012', expiresAtMs: expiry }}
          expired={false}
          display="123456-789012"
          progress={80}
          remaining={240000}
        />
      </div>
    )
  else {
    content = (
      <div className="m-auto w-full max-w-lg px-6 py-8">
        {step === 'setup' && (
          <EntryScreen
            onCreate={() => setStep('initialize')}
            onJoin={() => setStep('redeem')}
            onImport={() => setStep('import')}
          />
        )}
        {step === 'initialize' && (
          <InitializeSpaceScreen
            onSubmit={async () => ({ ok: true })}
            onBack={() => setStep('setup')}
          />
        )}
        {step === 'redeem' && (
          <RedeemInvitationScreen
            onSubmit={async () => ({ ok: true, redeem: null })}
            onBack={() => setStep('setup')}
          />
        )}
        {step === 'show-invitation' && (
          <ShowInvitationScreen code="123456789012" expiresAtMs={expiry} onCancel={noop} />
        )}
        {step === 'pending' && <JoinPendingScreen onCancel={noop} />}
        {step === 'rejected' && (
          <JoinRejectedScreen reason="authentication_rejected" onBack={noop} />
        )}
        {step === 'ready' && (
          <SpaceReadyScreen onInvite={async () => ({ ok: true })} onDone={noop} />
        )}
        {step === 'paired' && (
          <PairingCompleteScreen
            localDeviceName="Long local device name for layout verification"
            peerDeviceId="fixture-peer-123456789"
            onDone={noop}
          />
        )}
        {step === 'import' && <ImportConfigScreen onBack={() => setStep('setup')} />}
      </div>
    )
  }
  return (
    <main
      data-testid="typography-surface"
      className="flex h-screen min-h-0 flex-col overflow-auto bg-background text-foreground"
    >
      {content}
    </main>
  )
}

createRoot(document.getElementById('root')!).render(
  <Provider store={store}>
    <SettingContext.Provider value={settings}>
      <LazyMotion features={domMax} strict>
        <VisualEffectsProvider>
          <MemoryRouter>
            <TypographyFixture />
          </MemoryRouter>
        </VisualEffectsProvider>
      </LazyMotion>
    </SettingContext.Provider>
  </Provider>
)
