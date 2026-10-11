import { Hamburger, makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import type { SplitViewProps } from '@/ui-kit/contract'
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
 * WinUI NavigationView (Left mode) in a frameless window: the pane on the base layer with the
 * hamburger at its top, and the content as a card below a title-bar band that carries the title,
 * subtitle and commands. The band reserves the trailing width of the three caption buttons
 * (3 x 46 epx). With the pane hidden (LeftMinimal) the hamburger leads the band.
 */
const useStyles = makeStyles({
  root: {
    position: 'relative',
    height: '100%',
    display: 'flex',
    overflow: 'hidden',
  },
  sidebar: {
    width: '17.5rem',
    flexShrink: 0,
    display: 'flex',
    '& > [data-surface]': {
      flexGrow: 1,
      minWidth: 0,
      display: 'flex',
      flexDirection: 'column',
    },
  },
  sidebarChrome: {
    height: '3rem',
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    paddingInline: '0.25rem',
    ...dragRegion,
  },
  sidebarBody: {
    flexGrow: 1,
    minHeight: 0,
    overflowY: 'auto',
    display: 'flex',
    flexDirection: 'column',
    paddingInline: '0.25rem',
    paddingBottom: '0.5rem',
  },
  content: {
    flexGrow: 1,
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
  },
  band: {
    boxSizing: 'border-box',
    height: '3rem',
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: '0.75rem',
    paddingInlineStart: '1rem',
    paddingInlineEnd: '8.625rem',
    ...dragRegion,
  },
  bandWithToggle: { paddingInlineStart: '0.25rem' },
  title: {
    fontSize: tokens.fontSizeBase400,
    lineHeight: tokens.lineHeightBase400,
    fontWeight: tokens.fontWeightSemibold,
    whiteSpace: 'nowrap',
  },
  subtitle: {
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
    whiteSpace: 'nowrap',
  },
  spacer: { flexGrow: 1, minWidth: '0.5rem' },
  card: {
    flexGrow: 1,
    minHeight: 0,
    display: 'flex',
    paddingInlineEnd: '0.5rem',
    paddingBottom: '0.5rem',
    '& > [data-surface]': {
      flexGrow: 1,
      minWidth: 0,
      display: 'flex',
      overflow: 'hidden',
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
  toolbarLeading,
  toolbar,
  children,
}: SplitViewProps) {
  const styles = useStyles()
  // WinUI accelerator tooltips read "Label (Ctrl+B)"; the name stays the label alone.
  const toggle = (
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
  )
  return (
    <div className={styles.root} data-sidebar-hidden={sidebarHidden ? '' : undefined}>
      {sidebarHidden ? null : (
        <div className={styles.sidebar}>
          <Surface region="sidebar">
            <div className={styles.sidebarChrome}>{toggle}</div>
            <div className={styles.sidebarBody}>{sidebar}</div>
          </Surface>
        </div>
      )}
      <div className={styles.content}>
        <div className={mergeClasses(styles.band, sidebarHidden && styles.bandWithToggle)}>
          {sidebarHidden ? toggle : null}
          <span className={styles.title}>{title}</span>
          {subtitle ? <span className={styles.subtitle}>{subtitle}</span> : null}
          <span className={styles.spacer} />
          {toolbarLeading}
          {search}
          {toolbar}
        </div>
        <div className={styles.card}>
          <Surface region="content">
            <div className={styles.panes}>{children}</div>
          </Surface>
        </div>
      </div>
    </div>
  )
}
