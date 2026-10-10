import { LazyMotion, domMax } from 'framer-motion'
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import { createRoot } from 'react-dom/client'
import { MemoryRouter } from 'react-router'
import { CustomRelayMutationError } from '@/api/daemon'
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
import type { CustomRelay, CustomRelayMutation, SettingContextType } from '@/types/setting'
import { installSettingsFixtureEnvironment } from './settings-environment'
import '@/i18n'
import '@/styles/globals.css'

installSettingsFixtureEnvironment()

declare global {
  interface Window {
    __customRelayFixture?: {
      getMutations: () => CustomRelayMutation[]
      injectServerRelay: (relay: CustomRelay) => void
      removeServerRelay: (url: string) => void
      showLoadError: () => void
      setRestartFailure: (enabled: boolean) => void
    }
  }
}

function canonicalRelayUrl(value: string): string {
  let parsed: URL
  try {
    parsed = new URL(value.trim())
  } catch {
    throw new CustomRelayMutationError('invalidUrl')
  }
  if (!['http:', 'https:'].includes(parsed.protocol) || !parsed.hostname) {
    throw new CustomRelayMutationError('invalidUrl')
  }
  return parsed.toString()
}

function initialCustomRelays(): CustomRelay[] {
  if (new URLSearchParams(location.search).get('relayE2E') !== '1') return []
  return [
    { url: 'https://relay-one.example.com/', credentialConfigured: true },
    { url: 'https://relay-two.example.com/', credentialConfigured: false },
  ]
}
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
  const [customRelays, setCustomRelays] = useState<CustomRelay[]>(initialCustomRelays)
  const [relayError, setRelayError] = useState<string | null>(null)
  const serverRelaysRef = useRef<CustomRelay[]>(initialCustomRelays())
  const mutationsRef = useRef<CustomRelayMutation[]>([])

  const commitCustomRelays = useCallback((relays: CustomRelay[]) => {
    const next = relays.map(relay => ({ ...relay }))
    setCustomRelays(next)
    setSetting(previous => ({
      ...previous,
      network: {
        ...previous.network,
        customRelayUrls: next.map(relay => relay.url),
      },
    }))
  }, [])

  const reloadCustomRelays = useCallback(async () => {
    commitCustomRelays(serverRelaysRef.current)
    setRelayError(null)
  }, [commitCustomRelays])

  const mutateCustomRelay = useCallback(
    async (mutation: CustomRelayMutation) => {
      mutationsRef.current.push(structuredClone(mutation))
      const current = serverRelaysRef.current
      let next: CustomRelay[]

      if (mutation.action === 'add') {
        const url = canonicalRelayUrl(mutation.url)
        if (current.some(relay => relay.url === url)) {
          throw new CustomRelayMutationError('duplicate')
        }
        next = [
          ...current,
          {
            url,
            credentialConfigured: mutation.credential.action === 'set',
          },
        ]
      } else if (mutation.action === 'edit') {
        const index = current.findIndex(relay => relay.url === mutation.previousUrl)
        if (index === -1) {
          commitCustomRelays(current)
          throw new CustomRelayMutationError('notFound')
        }
        const url = canonicalRelayUrl(mutation.url)
        if (current.some((relay, relayIndex) => relayIndex !== index && relay.url === url)) {
          throw new CustomRelayMutationError('duplicate')
        }
        const previous = current[index]
        next = current.map((relay, relayIndex) =>
          relayIndex === index
            ? {
                url,
                credentialConfigured:
                  mutation.credential.action === 'keep'
                    ? previous.credentialConfigured
                    : mutation.credential.action === 'set',
              }
            : relay
        )
      } else {
        if (!current.some(relay => relay.url === mutation.url)) {
          commitCustomRelays(current)
          throw new CustomRelayMutationError('notFound')
        }
        next = current.filter(relay => relay.url !== mutation.url)
      }

      serverRelaysRef.current = next
      commitCustomRelays(next)
      setRelayError(null)
      return { relays: next, restartRequired: true }
    },
    [commitCustomRelays]
  )

  useEffect(() => {
    window.__customRelayFixture = {
      getMutations: () => structuredClone(mutationsRef.current),
      injectServerRelay: relay => {
        serverRelaysRef.current = [...serverRelaysRef.current, { ...relay }]
      },
      removeServerRelay: url => {
        serverRelaysRef.current = serverRelaysRef.current.filter(relay => relay.url !== url)
      },
      showLoadError: () => setRelayError('fixture load failure'),
      setRestartFailure: enabled => {
        window.__settingsFixtureNative!.restartShouldFail = enabled
      },
    }
    return () => {
      delete window.__customRelayFixture
    }
  }, [])

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
      customRelays,
      relayLoading: false,
      relayError,
      reloadSetting: async () => {},
      reloadCustomRelays,
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
      mutateCustomRelay,
    }
  }, [customRelays, mutateCustomRelay, relayError, reloadCustomRelays, setting])
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
