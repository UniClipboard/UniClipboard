import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  DEFAULT_CATEGORY,
  SETTINGS_CATEGORIES,
  type SettingsCategory,
} from '@/components/setting/settings-config'
import SettingsSidebar from '@/components/setting/SettingsSidebar'
import { ScrollArea } from '@/components/ui/scroll-area'
import { SidebarProvider, SidebarInset } from '@/components/ui/sidebar'
import { SettingContentLayout } from '@/layouts'
import { getSettingsIconPosition, startCircularCollapse } from '@/lib/theme-transition'
import { captureUserIntent } from '@/observability/breadcrumbs'

function SettingsPage() {
  const [activeCategory, setActiveCategory] = useState(DEFAULT_CATEGORY)
  const navigate = useNavigate()

  // Handle ESC key to navigate back with collapse animation
  useEffect(() => {
    captureUserIntent('open_settings')
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        const doNavigate = () => {
          const idx = (window.history.state as { idx?: number } | null)?.idx
          if (typeof idx === 'number' && idx > 0) {
            navigate(-1)
          } else {
            navigate('/')
          }
        }
        const { x, y } = getSettingsIconPosition()
        startCircularCollapse(x, y, doNavigate)
      }
    }
    window.addEventListener('keydown', handleEsc)
    return () => window.removeEventListener('keydown', handleEsc)
  }, [navigate])

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
          '--sidebar-width': '16rem',
        } as React.CSSProperties
      }
      className="min-h-0 h-full"
    >
      <SettingsSidebar activeCategory={activeCategory} onCategoryChange={handleCategoryChange} />
      <SidebarInset>
        <ScrollArea className="flex-1">
          <div className="flex-1 p-6">
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
