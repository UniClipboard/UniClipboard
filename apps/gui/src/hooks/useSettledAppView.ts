import { useEffect, useState } from 'react'
import type { AppContentView } from '@/components/app/app-content-state'

/** How long a settled view stays up while a brief startup gap is checked. */
export const POST_UNLOCK_HOLD_MS = 400

const SETTLED_VIEWS: ReadonlySet<AppContentView> = new Set(['unlock', 'authenticated'])
const STARTUP_VIEWS: ReadonlySet<AppContentView> = new Set(['startup', 'upgrade'])

/**
 * Unlocking re-runs the content-lock and space-readiness checks, which briefly resolve to the
 * startup view, both before and after the app view appears. Keep showing the unlock page or the
 * app through that short gap instead of flashing the startup screen; a slow check still reveals
 * it after the hold.
 */
export function useSettledAppView(view: AppContentView, holdMs = POST_UNLOCK_HOLD_MS) {
  const [shown, setShown] = useState(view)

  useEffect(() => {
    if (view === shown) return
    if (SETTLED_VIEWS.has(shown) && STARTUP_VIEWS.has(view)) {
      const timer = setTimeout(() => setShown(view), holdMs)
      return () => clearTimeout(timer)
    }
    setShown(view)
  }, [view, shown, holdMs])

  return shown
}
