import SettingsPage from '@ui-settings-page'
import { LazyMotion, domMax } from 'framer-motion'
import { useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import { Toaster } from '@/components/ui/toaster'
import { DeviceTrustProvider } from '@/contexts/DeviceTrustContext'
import { SettingContext } from '@/contexts/setting-context'
import { ShortcutContext, type ShortcutContextType } from '@/contexts/shortcut-context'
import { UpdateContext, type UpdateContextType } from '@/contexts/update-context'
import { store } from '@/store'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType } from '@/types/setting'
import { installSettingsFixtureEnvironment } from './settings-environment'
import '@/i18n'
import '@/styles/globals.css'

// Renders the real settings page, including its single persistent scroll area,
// so scroll behaviour across category switches matches the shipped window.
installSettingsFixtureEnvironment()

const shortcutContext: ShortcutContextType = {
  activeScope: 'settings',
  activeLayer: 'page',
  activePriority: 0,
  pushLayer: () => 'fixture',
  popLayer: () => {},
}

const updateContext: UpdateContextType = {
  state: { phase: 'idle', info: null, downloaded: 0, total: null },
  isCheckingUpdate: false,
  checkForUpdates: async () => null,
  downloadUpdate: async () => {},
  cancelDownload: async () => {},
  installUpdate: async () => {},
  updateInfo: null,
  downloadProgress: { phase: 'idle', downloaded: 0, total: null },
  installKind: null,
  isSystemManaged: false,
  isManualUpdate: false,
}

export default function SettingsPageFixture() {
  const [setting, setSetting] = useState(() => makeBaseSettings())
  const context = useMemo<SettingContextType>(() => {
    const patch = async (
      section:
        | 'general'
        | 'sync'
        | 'security'
        | 'retentionPolicy'
        | 'fileSync'
        | 'network'
        | 'quickPanel',
      value: object
    ) => {
      setSetting(previous => ({
        ...previous,
        [section]: { ...previous[section], ...value },
      }))
    }
    return {
      setting,
      loading: false,
      error: null,
      customRelays: [],
      relayLoading: false,
      relayError: null,
      reloadSetting: async () => {},
      reloadCustomRelays: async () => {},
      updateSetting: async next => setSetting(next),
      updateGeneralSetting: value => patch('general', value),
      updateAutostart: enabled => patch('general', { autoStart: enabled }),
      updateSyncSetting: value => patch('sync', value),
      updateSecuritySetting: value => patch('security', value),
      updateRetentionPolicy: value => patch('retentionPolicy', value),
      updateFileSyncSetting: value => patch('fileSync', value),
      updateKeyboardShortcuts: async (_, value) => {
        setSetting(previous => ({ ...previous, keyboardShortcuts: value }))
      },
      updateNetworkSetting: async value => {
        await patch('network', value)
        return { restartRequired: true }
      },
      updateQuickPanelSetting: async value => {
        if (window.__settingsFixtureNative!.quickPanelSaveShouldFail) {
          throw new Error('scripted quick panel save failure')
        }
        await patch('quickPanel', value)
        return { restartRequired: false }
      },
      mutateCustomRelay: async () => ({ relays: [], restartRequired: false }),
    }
  }, [setting])

  return (
    <SettingContext.Provider value={context}>
      <ShortcutContext value={shortcutContext}>
        <UpdateContext value={updateContext}>
          <LazyMotion features={domMax} strict>
            <VisualEffectsProvider>
              <MemoryRouter initialEntries={['/settings']}>
                <div className="h-screen min-h-0">
                  <SettingsPage />
                  <Toaster />
                </div>
              </MemoryRouter>
            </VisualEffectsProvider>
          </LazyMotion>
        </UpdateContext>
      </ShortcutContext>
    </SettingContext.Provider>
  )
}

createRoot(document.getElementById('root')!).render(
  <Provider store={store}>
    <DeviceTrustProvider enabled={false}>
      <SettingsPageFixture />
    </DeviceTrustProvider>
  </Provider>
)
