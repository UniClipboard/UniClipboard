import { LazyMotion, domMax } from 'framer-motion'
import { useEffect, useMemo, useState, type CSSProperties } from 'react'
import { createRoot } from 'react-dom/client'
import { MemoryRouter } from 'react-router'
import InsetSurface from '@/components/layout/InsetSurface'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import { SETTINGS_CATEGORIES } from '@/components/setting/settings-config'
import SettingsPageHeader from '@/components/setting/SettingsPageHeader'
import SettingsSidebar from '@/components/setting/SettingsSidebar'
import { SidebarProvider } from '@/components/ui/sidebar'
import { SettingContext } from '@/contexts/setting-context'
import { ShortcutContext, type ShortcutContextType } from '@/contexts/shortcut-context'
import { UpdateContext, type UpdateContextType } from '@/contexts/update-context'
import SettingContentLayout from '@/layouts/SettingContentLayout'
import { applyThemeOverrides, applyThemePreset } from '@/lib/theme-engine'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType } from '@/types/setting'
import { installSettingsFixtureEnvironment } from './settings-environment'
import '@/i18n'
import '@/styles/globals.css'

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

export default function SettingsFixture() {
  const [category, setCategory] = useState(
    () => new URLSearchParams(location.search).get('category') || 'general'
  )
  const [setting, setSetting] = useState(() =>
    makeBaseSettings({
      general: {
        theme: 'system',
        deviceName: 'Test computer',
        themeColorLight: 'zinc',
        themeColorDark: 'zinc',
        telemetryEnabled: false,
        usageAnalyticsEnabled: false,
      },
    })
  )
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
      setSetting(previous => ({ ...previous, [section]: { ...previous[section], ...value } }))
    }
    return {
      setting,
      loading: false,
      error: null,
      reloadSetting: async () => {},
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
        await patch('quickPanel', value)
        return { restartRequired: false }
      },
      saveRelay: async () => ({ restartRequired: false, credentialStatus: { configured: false } }),
    }
  }, [setting])
  const header = useMemo(() => <SettingsPageHeader category={category} />, [category])
  const Selected = SETTINGS_CATEGORIES.find(item => item.id === category)?.Component
  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      const mode =
        setting.general.theme === 'system'
          ? media.matches
            ? 'dark'
            : 'light'
          : setting.general.theme
      document.documentElement.classList.toggle('dark', mode === 'dark')
      applyThemePreset(
        mode === 'light' ? setting.general.themeColorLight : setting.general.themeColorDark,
        mode,
        document.documentElement
      )
      applyThemeOverrides(
        mode === 'light' ? setting.general.themeOverridesLight : setting.general.themeOverridesDark,
        document.documentElement
      )
      document.body.style.backgroundColor = 'var(--sidebar)'
    }
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [setting.general])
  return (
    <SettingContext.Provider value={context}>
      <ShortcutContext value={shortcutContext}>
        <UpdateContext value={updateContext}>
          <LazyMotion features={domMax} strict>
            <VisualEffectsProvider>
              <MemoryRouter>
                <SidebarProvider
                  className="h-screen min-h-0"
                  style={{ '--sidebar-width': '12rem' } as CSSProperties}
                >
                  <div className="hidden h-full md:block">
                    <SettingsSidebar activeCategory={category} onCategoryChange={setCategory} />
                  </div>
                  <InsetSurface className="mr-2 mb-2">
                    <main
                      data-testid="settings-scroll"
                      className="min-w-0 flex-1 overflow-y-auto p-4 text-foreground sm:p-6 lg:p-8"
                      key={category}
                    >
                      <SettingContentLayout header={header}>
                        {Selected && <Selected />}
                      </SettingContentLayout>
                    </main>
                  </InsetSurface>
                </SidebarProvider>
              </MemoryRouter>
            </VisualEffectsProvider>
          </LazyMotion>
        </UpdateContext>
      </ShortcutContext>
    </SettingContext.Provider>
  )
}
createRoot(document.getElementById('root')!).render(<SettingsFixture />)
