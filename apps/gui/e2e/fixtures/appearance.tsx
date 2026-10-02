// A real settings view with an isolated, in-memory backend for browser verification.
import { LazyMotion, domMax } from 'framer-motion'
import { useEffect, useMemo, useState, type CSSProperties } from 'react'
import { createRoot } from 'react-dom/client'
import { MemoryRouter } from 'react-router'
import { visualEffectsApi } from '@/api/visual-effects'
import InsetSurface from '@/components/layout/InsetSurface'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import AppearanceSection from '@/components/setting/AppearanceSection'
import SettingsPageHeader from '@/components/setting/SettingsPageHeader'
import SettingsSidebar from '@/components/setting/SettingsSidebar'
import { SidebarProvider } from '@/components/ui/sidebar'
import { SettingContext } from '@/contexts/setting-context'
import SettingContentLayout from '@/layouts/SettingContentLayout'
import { applyThemeOverrides, applyThemePreset } from '@/lib/theme-engine'
import { INITIAL_EFFECTS, initializeVisualEffects } from '@/lib/visual-effects-store'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { GeneralSettings, SettingContextType } from '@/types/setting'
import '@/i18n'
import '@/styles/globals.css'

let effects = { ...INITIAL_EFFECTS, sessionId: 'appearance-fixture', persistence: 'saved' as const }
visualEffectsApi.get = async () => effects
visualEffectsApi.subscribe = async () => () => {}
visualEffectsApi.environment = async (_, systemMotion) => {
  effects = { ...effects, systemMotion, revision: effects.revision + 1 }
  return effects
}
visualEffectsApi.setMode = async mode => {
  effects = {
    ...effects,
    mode,
    lowEffects: mode !== 'effects',
    reduceMotion: mode !== 'effects',
    reason: mode === 'auto' ? 'platform_default' : 'manual',
    revision: effects.revision + 1,
  }
  return effects
}
initializeVisualEffects()

export default function AppearanceFixture() {
  const header = useMemo(() => <SettingsPageHeader category="appearance" />, [])
  const [setting, setSetting] = useState(() =>
    makeBaseSettings({
      general: { theme: 'system', themeColorLight: 'zinc', themeColorDark: 'zinc' },
    })
  )
  const context = useMemo(
    () =>
      ({
        setting,
        updateGeneralSetting: async (patch: Partial<GeneralSettings>) => {
          setSetting(previous => ({ ...previous, general: { ...previous.general, ...patch } }))
        },
      }) as SettingContextType,
    [setting]
  )
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
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
      document.body.style.backgroundColor = 'var(--background)'
      document.body.style.minHeight = '100vh'
    }
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [setting.general])
  return (
    <SettingContext.Provider value={context}>
      <LazyMotion features={domMax} strict>
        <VisualEffectsProvider>
          <MemoryRouter>
            <SidebarProvider
              className="h-screen min-h-0 bg-sidebar"
              style={{ '--sidebar-width': '12rem' } as CSSProperties}
            >
              <div className="hidden h-full md:block">
                <SettingsSidebar activeCategory="appearance" onCategoryChange={() => {}} />
              </div>
              <InsetSurface className="mr-2 mb-2">
                <main className="min-w-0 flex-1 overflow-y-auto p-4 text-foreground sm:p-6 lg:p-8">
                  <SettingContentLayout header={header}>
                    <AppearanceSection />
                  </SettingContentLayout>
                </main>
              </InsetSurface>
            </SidebarProvider>
          </MemoryRouter>
        </VisualEffectsProvider>
      </LazyMotion>
    </SettingContext.Provider>
  )
}
createRoot(document.getElementById('root')!).render(<AppearanceFixture />)
