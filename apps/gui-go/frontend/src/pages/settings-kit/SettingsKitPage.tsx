import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useHistoryLayoutTier } from '@/components/history/layout/history-layout'
import { SETTINGS_CATEGORIES } from '@/components/setting/settings-config'
import { useSettingsPage } from '@/components/setting/useSettingsPage'
import { PortalScope } from '@/components/ui/portal-scope'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { useMacTrafficLightPosition } from '@/hooks/useMacTrafficLightPosition'
import {
  MAC_CONTENT_TRAFFIC_LIGHT_OFFSET,
  MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET,
} from '@/layouts/mac-traffic-light-offsets'
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
 * Settings on the platform kit (macOS design MS1, Windows design SetWin): a sidebar of categories
 * beside one scrolling column of setting groups. The categories, their sections and every save and
 * restart rule are the shared ones of `SettingsPage`; this view only draws them, and the skin
 * decides how. Chosen at build time through `@ui-settings-page`, never branched on the OS here.
 */
function SettingsKitPage() {
  const { t } = useTranslation()
  const colorScheme = useAppliedColorScheme()
  const compact = useHistoryLayoutTier() === 'compact'
  const [drawerOpen, setDrawerOpen] = useState(false)
  const sidebarHidden = compact && !drawerOpen
  // A resized window starts with its default navigation layout.
  useEffect(() => {
    const closeDrawer = () => setDrawerOpen(false)
    window.addEventListener('resize', closeDrawer)
    return () => window.removeEventListener('resize', closeDrawer)
  }, [])
  const { activeCategory, activeCategoryConfig, handleCategoryChange, goBack, viewportRef } =
    useSettingsPage({ backShortcutEnabled: !drawerOpen })
  // Where the sections' own popups open, inside the themed root instead of <body>.
  const [popupHost, setPopupHost] = useState<HTMLDivElement | null>(null)
  useMacTrafficLightPosition(
    sidebarHidden ? MAC_CONTENT_TRAFFIC_LIGHT_OFFSET : MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET
  )

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
        sidebarHidden={sidebarHidden}
        sidebarOverlay={compact}
        sidebarToggle={
          compact
            ? {
                label: t(sidebarHidden ? 'history.sidebar.show' : 'history.sidebar.hide'),
                onClick: () => setDrawerOpen(open => !open),
              }
            : undefined
        }
        sidebar={
          <SidebarNav
            label={t('nav.settings')}
            sections={[{ id: 'categories', items }]}
            footer={[{ id: BACK_ID, label: t('nav.back'), icon: 'back', shortcutHint: 'Esc' }]}
            value={activeCategory}
            onValueChange={id => {
              setDrawerOpen(false)
              if (id === BACK_ID) goBack()
              else handleCategoryChange(id)
            }}
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
      <div className="settings-popups-anchor">
        <div ref={setPopupHost} className="settings-popups" />
      </div>
    </UiRoot>
  )
}

/** The split view reaches the window top (traffic lights, or the app-drawn controls), as the history page. */
export const settingsOwnsWindowTop = true

export default SettingsKitPage
