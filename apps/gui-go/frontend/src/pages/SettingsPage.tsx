import { useMemo } from 'react'
import InsetSurface from '@/components/layout/InsetSurface'
import SettingsPageHeader from '@/components/setting/SettingsPageHeader'
import SettingsSidebar from '@/components/setting/SettingsSidebar'
import { useSettingsPage } from '@/components/setting/useSettingsPage'
import { ScrollArea } from '@/components/ui/scroll-area'
import { SidebarProvider, SidebarInset } from '@/components/ui/sidebar'
import { SettingContentLayout } from '@/layouts'

function SettingsPage() {
  const { activeCategory, activeCategoryConfig, handleCategoryChange, viewportRef } =
    useSettingsPage()
  const ActiveSection = activeCategoryConfig?.Component
  const sectionHeader = useMemo(
    () => (activeCategoryConfig ? <SettingsPageHeader category={activeCategoryConfig.id} /> : null),
    [activeCategoryConfig]
  )

  const content = (
    <SidebarInset className="min-h-0 bg-transparent">
      <ScrollArea className="flex-1 min-h-0" viewportRef={viewportRef}>
        <div className="p-4 sm:p-6 lg:p-8">
          {ActiveSection && (
            <SettingContentLayout header={sectionHeader}>
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
      <SettingsSidebar activeCategory={activeCategory} onCategoryChange={handleCategoryChange} />
      <InsetSurface className="mr-2 mb-2 rounded-xl">{content}</InsetSurface>
    </SidebarProvider>
  )
}

/** The page draws no window top of its own, so the layout supplies the title bar. */
export const settingsOwnsWindowTop = false

export default SettingsPage
