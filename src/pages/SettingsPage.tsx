import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import InsetSurface from '@/components/layout/InsetSurface'
import {
  DEFAULT_CATEGORY,
  SETTINGS_CATEGORIES,
  type SettingsCategory,
} from '@/components/setting/settings-config'
import {
  readSettingsScrollOffset,
  rememberSettingsScrollOffset,
} from '@/components/setting/settings-scroll-session'
import SettingsPageHeader from '@/components/setting/SettingsPageHeader'
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

  const viewportRef = useRef<HTMLDivElement | null>(null)

  // One scroll area serves every category, so its offset carries over unless the
  // outgoing category's offset is stored and the incoming one's is restored.
  useLayoutEffect(() => {
    const viewport = viewportRef.current
    if (!viewport) return
    const limit = Math.max(0, viewport.scrollHeight - viewport.clientHeight)
    viewport.scrollTop = Math.min(readSettingsScrollOffset(activeCategory), limit)
    // Recorded while scrolling, which also covers leaving the settings page.
    // Reading the offset on teardown instead would already see it clamped by the
    // incoming category's shorter content.
    const record = () => rememberSettingsScrollOffset(activeCategory, viewport.scrollTop)
    viewport.addEventListener('scroll', record, { passive: true })
    return () => viewport.removeEventListener('scroll', record)
  }, [activeCategory])

  const handleCategoryChange = (category: string) => {
    // Scroll events are delivered at a later rendering step, so a switch in the
    // same frame as the last scroll would otherwise store a stale offset.
    const viewport = viewportRef.current
    if (viewport) rememberSettingsScrollOffset(activeCategory, viewport.scrollTop)
    setActiveCategory(category)
  }

  const activeCategoryConfig = SETTINGS_CATEGORIES.find(
    (cat: SettingsCategory) => cat.id === activeCategory
  )
  const ActiveSection = activeCategoryConfig?.Component
  const sectionHeader = useMemo(
    () => (activeCategoryConfig ? <SettingsPageHeader category={activeCategoryConfig.id} /> : null),
    [activeCategoryConfig]
  )

  const { isLinux, isTauri } = usePlatform()
  const useFlatLayout = isLinux && isTauri

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
        <InsetSurface className="mr-2 mb-2 rounded-xl">{content}</InsetSurface>
      )}
    </SidebarProvider>
  )
}

export default SettingsPage
