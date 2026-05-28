import { useEffect, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import InsetSurface from '@/components/layout/InsetSurface'
import {
  DEFAULT_CATEGORY,
  SETTINGS_CATEGORIES,
  type SettingsCategory,
} from '@/components/setting/settings-config'
import SettingsSidebar from '@/components/setting/SettingsSidebar'
import { ScrollArea } from '@/components/ui/scroll-area'
import { SidebarProvider, SidebarInset } from '@/components/ui/sidebar'
import { usePlatform } from '@/hooks/usePlatform'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { SettingContentLayout } from '@/layouts'
import { captureUserIntent } from '@/observability/breadcrumbs'

function SettingsPage() {
  const routerLocation = useLocation()
  const { state: locationState, pathname: locationPathname } = routerLocation
  const [activeCategory, setActiveCategory] = useState(
    (locationState as { category?: string } | null)?.category || DEFAULT_CATEGORY
  )
  const navigate = useNavigate()
  useShortcutScope('settings')

  useShortcut({
    key: 'esc',
    scope: 'settings',
    handler: () => {
      const idx = (window.history.state as { idx?: number } | null)?.idx
      if (typeof idx === 'number' && idx > 0) {
        navigate(-1)
      } else {
        navigate('/')
      }
    },
  })

  // Handle ESC key to navigate back with collapse animation
  useEffect(() => {
    captureUserIntent('open_settings')
  }, [])

  useEffect(() => {
    if (locationState && (locationState as { category?: string }).category) {
      const newState = { ...locationState } as Record<string, unknown>
      delete newState.category
      navigate(locationPathname, { replace: true, state: newState })
    }
  }, [locationState, navigate, locationPathname])

  const handleCategoryChange = (category: string) => {
    setActiveCategory(category)
  }

  const activeCategoryConfig = SETTINGS_CATEGORIES.find(
    (cat: SettingsCategory) => cat.id === activeCategory
  )
  const ActiveSection = activeCategoryConfig?.Component

  const { isLinux, isTauri } = usePlatform()
  const useFlatLayout = isLinux && isTauri

  const content = (
    <SidebarInset className="min-h-0 bg-transparent">
      <ScrollArea className="flex-1 min-h-0">
        <div className="p-6">
          {ActiveSection && (
            <SettingContentLayout>
              <ActiveSection />
            </SettingContentLayout>
          )}
        </div>
      </ScrollArea>
    </SidebarInset>
  )

  return (
    <SidebarProvider
      style={
        {
          '--sidebar-width': '12rem',
        } as React.CSSProperties
      }
      className="min-h-0 h-full"
    >
      <SettingsSidebar
        activeCategory={activeCategory}
        onCategoryChange={handleCategoryChange}
        flat={useFlatLayout}
      />
      {useFlatLayout ? (
        <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-card text-card-foreground">
          {content}
        </main>
      ) : (
        <InsetSurface className="mr-2 mb-2">{content}</InsetSurface>
      )}
    </SidebarProvider>
  )
}

export default SettingsPage
