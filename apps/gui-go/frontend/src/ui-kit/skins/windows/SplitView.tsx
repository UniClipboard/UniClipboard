import { Hamburger, makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import type { SplitViewProps } from '@/ui-kit/contract'
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

/**
 * WinUI NavigationView (Left mode) under a custom title bar in a frameless window (Win11 boards
 * C2 and WA2): one 48 px title bar across the window with the hamburger at the far left, the
 * toolbar centred and the room of the app-drawn caption buttons at the right (the main layout
 * publishes it as `--window-controls-inset-x`); below it the 260 px pane on the base layer and the
 * content card, whose leading top corner is rounded. The title and subtitle head the list pane
 * (`Pane kind="list"`), as in WA2. With the pane hidden (LeftMinimal) only the hamburger remains.
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
    gridTemplateColumns: '1fr minmax(0, auto) 1fr',
    alignItems: 'center',
    paddingInlineStart: '0.25rem',
    paddingInlineEnd: 'var(--window-controls-inset-x, 0px)',
    ...dragRegion,
  },
  toggle: { justifySelf: 'start' },
  toolbar: {
    minWidth: 0,
    display: 'flex',
    justifyContent: 'center',
    // In the title bar the toolbar is part of the chrome, not a CommandBar card of its own.
    '& [role="toolbar"]': { backgroundColor: 'transparent', border: 'none', padding: 0 },
    // Board C2: the search field is about 460 px wide in a 1280 px window.
    '& [data-search-field]': { width: '28.75rem', maxWidth: '100%', flexShrink: 1 },
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
  cardAlone: {
    '& > [data-surface]': { borderInlineStartWidth: 0, borderStartStartRadius: 0 },
  },
  panes: { flexGrow: 1, minWidth: 0, minHeight: 0, display: 'flex' },
})

export function SplitView({
  sidebar,
  sidebarHidden,
  sidebarToggle,
  title,
  subtitle,
  toolbar,
  children,
}: SplitViewProps) {
  const styles = useStyles()
  return (
    <div className={styles.root} data-sidebar-hidden={sidebarHidden ? '' : undefined}>
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
              aria-label={sidebarToggle.label}
              aria-expanded={!sidebarHidden}
              onClick={() => sidebarToggle.onClick()}
            />
          </WithTooltip>
        </span>
        <div className={styles.toolbar}>{toolbar}</div>
      </div>
      <div className={styles.body}>
        {sidebarHidden ? null : (
          <div className={styles.sidebar}>
            <Surface region="sidebar">{sidebar}</Surface>
          </div>
        )}
        <div className={mergeClasses(styles.card, sidebarHidden && styles.cardAlone)}>
          <Surface region="content">
            <SplitViewTitle value={{ title, subtitle }}>
              <div className={styles.panes}>{children}</div>
            </SplitViewTitle>
          </Surface>
        </div>
      </div>
    </div>
  )
}
