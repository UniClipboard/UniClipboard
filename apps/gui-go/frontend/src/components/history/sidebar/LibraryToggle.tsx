import { m } from 'framer-motion'
import { ListIndentIncrease } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useLibraryChrome } from '@/contexts/library-chrome-context'
import { cn } from '@/lib/utils'
import { LIBRARY_SIDEBAR_TRANSITION, LIBRARY_TOGGLE_LAYOUT_ID } from './history-sidebar-motion'

interface LibraryToggleButtonProps {
  /** Glide between the expanded strip and the icon column (shared layout).
   * The overlay's copy opts out: it sits over the rail's own toggle. */
  shared?: boolean
  onPointerEnter?: () => void
  onPointerLeave?: () => void
}

/** Show/hide the Library sidebar (or its compact-tier drawer). */
export function LibraryToggleButton({
  shared = true,
  onPointerEnter,
  onPointerLeave,
}: LibraryToggleButtonProps) {
  const { t } = useTranslation()
  const { hidden, drawerOpen, toggle } = useLibraryChrome()
  const shown = drawerOpen || !hidden
  const label = t(shown ? 'history.sidebar.hide' : 'history.sidebar.show')
  return (
    <m.button
      type="button"
      layoutId={shared ? LIBRARY_TOGGLE_LAYOUT_ID : undefined}
      transition={LIBRARY_SIDEBAR_TRANSITION}
      aria-label={label}
      aria-expanded={shown}
      title={`${label}  ⌃⌘S`}
      onClick={toggle}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
      className="flex size-7 shrink-0 items-center justify-center rounded-full text-foreground/70 transition-colors hover:bg-foreground/8 hover:text-foreground"
    >
      {/* Collapsed: lines with a chevron pointing out (show). Expanded: the
          mirror image, the chevron at the lines' end pointing back (hide). */}
      <ListIndentIncrease className={cn('size-4', shown && '-scale-x-100')} aria-hidden="true" />
    </m.button>
  )
}
