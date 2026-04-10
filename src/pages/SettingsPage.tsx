import { useEffect, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
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
import { cn } from '@/lib/utils'
import { captureUserIntent } from '@/observability/breadcrumbs'

function SettingsPage() {
  const location = useLocation()
  const { isWindows } = usePlatform()
  const [activeCategory, setActiveCategory] = useState(
    (location.state as { category?: string } | null)?.category || DEFAULT_CATEGORY
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
    if (location.state && (location.state as { category?: string }).category) {
      const newState = { ...location.state } as Record<string, unknown>
      delete newState.category
      navigate(location.pathname, { replace: true, state: newState })
    }
  }, [location.state, navigate, location.pathname])

  const handleCategoryChange = (category: string) => {
    setActiveCategory(category)
  }

  const activeCategoryConfig = SETTINGS_CATEGORIES.find(
    (cat: SettingsCategory) => cat.id === activeCategory
  )
  const ActiveSection = activeCategoryConfig?.Component

  return (
    <SidebarProvider
      style={
        {
          '--sidebar-width': '12rem',
        } as React.CSSProperties
      }
      className="min-h-0 h-full"
    >
      <SettingsSidebar activeCategory={activeCategory} onCategoryChange={handleCategoryChange} />
      <SidebarInset
        className={cn(
          'min-h-0',
          isWindows &&
            'rounded-tl-[22px] bg-background/92 shadow-[inset_1px_1px_0_rgba(255,255,255,0.18)] backdrop-blur-sm'
        )}
      >
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
    </SidebarProvider>
  )
}

export default SettingsPage
