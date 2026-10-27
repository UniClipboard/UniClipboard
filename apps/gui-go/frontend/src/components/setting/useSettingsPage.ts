import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import {
  DEFAULT_CATEGORY,
  SETTINGS_CATEGORIES,
  type SettingsCategory,
} from '@/components/setting/settings-config'
import {
  readSettingsScrollOffset,
  rememberSettingsScrollOffset,
} from '@/components/setting/settings-scroll-session'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { captureUserIntent } from '@/observability/breadcrumbs'

/**
 * Page behaviour shared by every presentation of Settings: the active category (optionally opened
 * from navigation state), Esc and back navigation, and the per-category scroll offset. The views
 * (`pages/SettingsPage.tsx`, `pages/settings-kit/SettingsKitPage.tsx`) only draw it.
 */
export function useSettingsPage({ backShortcutEnabled = true } = {}) {
  const routerLocation = useLocation()
  const { state: locationState, pathname: locationPathname } = routerLocation
  const [activeCategory, setActiveCategory] = useState(
    (locationState as { category?: string } | null)?.category || DEFAULT_CATEGORY
  )
  const navigate = useNavigate()
  useShortcutScope('settings')

  const goBack = () => {
    const idx = (window.history.state as { idx?: number } | null)?.idx
    if (typeof idx === 'number' && idx > 0) {
      navigate(-1)
    } else {
      navigate('/')
    }
  }

  useShortcut({ key: 'esc', scope: 'settings', handler: goBack, enabled: backShortcutEnabled })

  useEffect(() => {
    captureUserIntent('open_settings')
  }, [])

  useEffect(() => {
    if (locationState && (locationState as { category?: string }).category) {
      const newState = { ...locationState } as Record<string, unknown>
      // A startup recovery action can target Settings while it is already mounted.
      setActiveCategory(String(newState.category))
      delete newState.category
      navigate(locationPathname, { replace: true, state: newState })
    }
  }, [locationState, navigate, locationPathname])

  const viewportRef = useRef<HTMLDivElement | null>(null)
  // The offset the active category was restored to. A viewport still sitting on
  // it has not been moved by the user, and a restore clamped by a temporarily
  // taller window must not shrink the stored offset.
  const restoredOffsetRef = useRef(0)

  // One scroll area serves every category, so its offset carries over unless the
  // outgoing category's offset is stored and the incoming one's is restored.
  useLayoutEffect(() => {
    const viewport = viewportRef.current
    if (!viewport) return
    const limit = Math.max(0, viewport.scrollHeight - viewport.clientHeight)
    const restored = Math.min(readSettingsScrollOffset(activeCategory), limit)
    restoredOffsetRef.current = restored
    viewport.scrollTop = restored
    // Recorded while scrolling, which also covers leaving the settings page.
    // Reading the offset on teardown instead would already see it clamped by the
    // incoming category's shorter content.
    const record = () => {
      if (viewport.scrollTop === restoredOffsetRef.current) return
      rememberSettingsScrollOffset(activeCategory, viewport.scrollTop)
    }
    viewport.addEventListener('scroll', record, { passive: true })
    return () => viewport.removeEventListener('scroll', record)
  }, [activeCategory])

  const handleCategoryChange = (category: string) => {
    // Scroll events are delivered at a later rendering step, so a switch in the
    // same frame as the last scroll would otherwise store a stale offset.
    const viewport = viewportRef.current
    if (viewport && viewport.scrollTop !== restoredOffsetRef.current) {
      rememberSettingsScrollOffset(activeCategory, viewport.scrollTop)
    }
    setActiveCategory(category)
  }

  const activeCategoryConfig = SETTINGS_CATEGORIES.find(
    (cat: SettingsCategory) => cat.id === activeCategory
  )

  return { activeCategory, activeCategoryConfig, handleCategoryChange, goBack, viewportRef }
}
