import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SETTINGS_CATEGORIES } from '@/components/setting/settings-config'
import { useSettingsPage } from '@/components/setting/useSettingsPage'
import { PortalScope } from '@/components/ui/portal-scope'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { useMacTrafficLightPosition } from '@/hooks/useMacTrafficLightPosition'
import { MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET } from '@/layouts/mac-traffic-light-offsets'
import {
  Pane,
  SettingsContent,
  SidebarNav,
  SplitView,
  UiRoot,
  type IconName,
  type SidebarNavItem,
} from '@/ui-kit'

// Skin glyphs standing in for each category's icon (the kit's icon set is closed).
const CATEGORY_ICONS: Record<string, IconName> = {
  general: 'settings',
  appearance: 'sun',
  shortcuts: 'edit',
  quickPanel: 'list',
  sync: 'retry',
  security: 'shield',
  network: 'globe',
  storage: 'package',
  about: 'info',
}

const BACK_ID = 'back'

/**
 * Settings on the macOS skin (design MS1): a source-list sidebar of categories beside one scrolling
 * column of inset groups. The categories, their sections and every save and restart rule are the
 * shared ones of `SettingsPage`; this view only draws them. Chosen at build time through
 * `@ui-settings-page`, never branched on the OS here.
 */
function SettingsKitPage() {
  const { t } = useTranslation()
  const colorScheme = useAppliedColorScheme()
  const { activeCategory, activeCategoryConfig, handleCategoryChange, goBack, viewportRef } =
    useSettingsPage()
  // Where the sections' own popups open, inside the themed root instead of <body>.
  const [popupHost, setPopupHost] = useState<HTMLDivElement | null>(null)
  useMacTrafficLightPosition(MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET)

  const items: SidebarNavItem[] = SETTINGS_CATEGORIES.map(category => ({
    id: category.id,
    label: t(`settings.categories.${category.id}`),
    icon: CATEGORY_ICONS[category.id],
  }))
  const ActiveSection = activeCategoryConfig?.Component
  const title = t(`settings.categories.${activeCategory}`)

  return (
    <UiRoot colorScheme={colorScheme}>
      <SplitView
        sidebar={
          <SidebarNav
            label={t('nav.settings')}
            sections={[{ id: 'categories', items }]}
            footer={[{ id: BACK_ID, label: t('nav.back'), icon: 'back', shortcutHint: 'Esc' }]}
            value={activeCategory}
            onValueChange={id => (id === BACK_ID ? goBack() : handleCategoryChange(id))}
          />
        }
        title={title}
      >
        <Pane kind="main" label={title}>
          <PortalScope value={popupHost}>
            <SettingsContent label={title} viewportRef={viewportRef}>
              {ActiveSection && <ActiveSection />}
            </SettingsContent>
          </PortalScope>
        </Pane>
      </SplitView>
      <div className="mac-settings-popups-anchor">
        <div ref={setPopupHost} className="mac-settings-popups" />
      </div>
    </UiRoot>
  )
}

/** The split view reaches the window top (traffic lights over the sidebar), as the history page. */
export const settingsOwnsWindowTop = true

export default SettingsKitPage
