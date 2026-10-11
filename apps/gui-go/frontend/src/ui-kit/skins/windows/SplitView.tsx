import { Hamburger, makeStyles, tokens } from '@fluentui/react-components'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { KeyboardEvent } from 'react'
import type { SplitViewProps } from '@/ui-kit/contract'
import { SidebarDisplayProvider } from './sidebar-display'
import { SplitViewTitle } from './split-view-title'
import { Surface } from './Surface'
import { WithTooltip } from './WithTooltip'

// Wails reads drag regions from this CSS property; interactive controls opt out.
const noDrag = { '--wails-draggable': 'no-drag' }
const dragRegion = {
  '--wails-draggable': 'drag',
  '& button': noDrag,
  '& input': noDrag,
  '& [role="combobox"]': noDrag,
  '& [role="toolbar"]': noDrag,
}

// Below this width the three columns do not fit beside an expanded pane (it matches the app's compact window tier),
// so an expanded pane opens over the content instead, as NavigationView does below ExpandedModeThresholdWidth.
const OVERLAY_BELOW = 1100

/**
 * WinUI NavigationView under a custom title bar in a frameless window (Win11 boards
 * C2 and WA2): one 48 px title bar across the window with the hamburger at the far left, the
 * search field centred, the toolbar commands at the trailing end and the room of the app-drawn
 * caption buttons at the right (the main layout
 * publishes it as `--window-controls-inset-x`); below it the 260 px pane on the base layer and the
 * content card, whose leading top corner is rounded. The title and subtitle head the list pane
 * (`Pane kind="list"`), as in WA2. NavigationView's display modes follow the window: a hidden sidebar is the
 * 48 px icon rail (LeftCompact, board WA3); an expanded sidebar in a narrow window opens over the content with
 * light dismiss (board WA4): a click outside or Escape asks `sidebarToggle` to close it, and focus moves into the
 * pane and back to the hamburger.
 */
const useStyles = makeStyles({
  root: {
    position: 'relative',
    height: '100%',
    display: 'flex',
    flexDirection: 'column',
    overflow: 'hidden',
  },
  titleBar: {
    boxSizing: 'border-box',
    height: '3rem',
    flexShrink: 0,
    display: 'grid',
    // The search column gives way first, so the commands at the end are never clipped.
    gridTemplateColumns: 'minmax(max-content, 1fr) minmax(0, 28.75rem) minmax(max-content, 1fr)',
    alignItems: 'center',
    paddingInlineStart: '0.25rem',
    paddingInlineEnd: 'var(--window-controls-inset-x, 0px)',
    ...dragRegion,
  },
  toggle: { justifySelf: 'start' },
  search: {
    minWidth: 0,
    display: 'flex',
    justifyContent: 'center',
    // Board C2: the search field is about 460 px wide in a 1280 px window.
    '& [data-search-field]': { width: '100%' },
  },
  commands: {
    justifySelf: 'end',
    minWidth: 0,
    display: 'flex',
    paddingInline: '0.5rem',
    // In the title bar the toolbar is part of the chrome, not a CommandBar card of its own.
    '& [role="toolbar"]': { backgroundColor: 'transparent', border: 'none', padding: 0 },
  },
  body: { flexGrow: 1, minHeight: 0, display: 'flex' },
  sidebar: {
    width: '16.25rem',
    flexShrink: 0,
    display: 'flex',
    '& > [data-surface]': {
      flexGrow: 1,
      minWidth: 0,
      display: 'flex',
      flexDirection: 'column',
      overflowY: 'auto',
      paddingBlock: '0.25rem 0.625rem',
      paddingInline: '0.375rem',
    },
  },
  rail: {
    width: '3rem',
    flexShrink: 0,
    display: 'flex',
    '& > [data-surface]': { flexGrow: 1, minWidth: 0, display: 'flex', flexDirection: 'column' },
  },
  // Board WA4: the open pane floats over the content below the title bar, with the flyout elevation.
  overlay: {
    position: 'absolute',
    insetInlineStart: 0,
    top: '3rem',
    bottom: 0,
    zIndex: 3,
    width: '17.5rem',
    display: 'flex',
    boxShadow: tokens.shadow28,
    borderStartEndRadius: tokens.borderRadiusXLarge,
    overflow: 'hidden',
    '& > [data-surface]': {
      flexGrow: 1,
      minWidth: 0,
      display: 'flex',
      flexDirection: 'column',
      overflowY: 'auto',
      paddingBlock: '0.25rem 0.625rem',
      paddingInline: '0.375rem',
    },
  },
  lightDismiss: { position: 'absolute', inset: '3rem 0 0 0', zIndex: 2 },
  card: {
    flexGrow: 1,
    minWidth: 0,
    minHeight: 0,
    display: 'flex',
    '& > [data-surface]': {
      flexGrow: 1,
      minWidth: 0,
      display: 'flex',
      overflow: 'hidden',
      borderInlineEndWidth: 0,
      borderBottomWidth: 0,
      borderRadius: 0,
      borderStartStartRadius: tokens.borderRadiusXLarge,
    },
  },

  panes: { flexGrow: 1, minWidth: 0, minHeight: 0, display: 'flex' },
})

export function SplitView({
  sidebar,
  sidebarHidden,
  sidebarToggle,
  title,
  subtitle,
  search,
  toolbar,
  children,
}: SplitViewProps) {
  const styles = useStyles()
  const root = useRef<HTMLDivElement>(null)
  const toggle = useRef<HTMLButtonElement>(null)
  const pane = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  useLayoutEffect(() => {
    const element = root.current
    if (!element) return
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width))
    observer.observe(element)
    return () => observer.disconnect()
  }, [])
  const overlay = !sidebarHidden && width > 0 && width < OVERLAY_BELOW
  const wasOverlay = useRef(false)
  useEffect(() => {
    if (overlay) {
      const target =
        pane.current?.querySelector<HTMLElement>('[aria-current="page"]') ??
        pane.current?.querySelector<HTMLElement>('button, [tabindex="0"]')
      target?.focus()
    } else if (wasOverlay.current) {
      toggle.current?.focus()
    }
    wasOverlay.current = overlay
  }, [overlay])
  const dismiss = () => sidebarToggle.onClick()
  const onPaneKeyDown = (event: KeyboardEvent) => {
    if (event.key !== 'Escape') return
    event.stopPropagation()
    dismiss()
  }
  return (
    <div
      ref={root}
      className={styles.root}
      data-sidebar-hidden={sidebarHidden ? '' : undefined}
      data-sidebar-display={sidebarHidden ? 'compact' : overlay ? 'overlay' : 'expanded'}
    >
      <div className={styles.titleBar}>
        <span className={styles.toggle}>
          {/* WinUI accelerator tooltips read "Label (Ctrl+B)"; the name stays the label alone. */}
          <WithTooltip
            content={
              sidebarToggle.shortcutHint
                ? `${sidebarToggle.label} (${sidebarToggle.shortcutHint})`
                : sidebarToggle.label
            }
            relationship="inaccessible"
          >
            <Hamburger
              ref={toggle}
              aria-label={sidebarToggle.label}
              aria-expanded={!sidebarHidden}
              onClick={() => sidebarToggle.onClick()}
            />
          </WithTooltip>
        </span>
        <div className={styles.search}>{search}</div>
        <div className={styles.commands}>{toolbar}</div>
      </div>
      <div className={styles.body}>
        {sidebarHidden || overlay ? (
          <div className={styles.rail}>
            <Surface region="sidebar">
              {/* While the pane is open over the content the rail under it stays empty: one sidebar at a time. */}
              {sidebarHidden ? (
                <SidebarDisplayProvider value="compact">{sidebar}</SidebarDisplayProvider>
              ) : null}
            </Surface>
          </div>
        ) : (
          <div className={styles.sidebar}>
            <Surface region="sidebar">{sidebar}</Surface>
          </div>
        )}
        <div className={styles.card}>
          <Surface region="content">
            <SplitViewTitle value={{ title, subtitle }}>
              <div className={styles.panes}>{children}</div>
            </SplitViewTitle>
          </Surface>
        </div>
      </div>
      {overlay ? (
        <>
          <div className={styles.lightDismiss} aria-hidden="true" onMouseDown={dismiss} />
          {/* oxlint-disable-next-line jsx-a11y/no-static-element-interactions -- Escape closes the light-dismiss pane; the pane's own controls take focus */}
          <div ref={pane} className={styles.overlay} onKeyDown={onPaneKeyDown}>
            <Surface region="sidebar">{sidebar}</Surface>
          </div>
        </>
      ) : null}
    </div>
  )
}
