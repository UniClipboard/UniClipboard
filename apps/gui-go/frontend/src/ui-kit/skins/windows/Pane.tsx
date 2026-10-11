import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import { useState } from 'react'
import type { PaneProps } from '@/ui-kit/contract'
import { PaneSlotsProvider } from './pane-slots'
import { useSplitViewTitle } from './split-view-title'

// Columns of the content card (board WA2): the list column is a 480 px master column that may
// shrink, headed by the view title; the detail column takes the rest on a slightly darker layer.
// A divider separates them, as between WinUI ListDetails columns.
const useStyles = makeStyles({
  pane: {
    position: 'relative',
    boxSizing: 'border-box',
    minWidth: 0,
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
  },
  list: {
    flex: '0 1 30rem',
    minWidth: '18rem',
    borderInlineEndWidth: tokens.strokeWidthThin,
    borderInlineEndStyle: 'solid',
    borderInlineEndColor: 'var(--w11-divider)',
  },
  detail: { flex: '1 1 21rem', minWidth: '18rem', backgroundColor: 'var(--w11-detail)' },
  slot: { display: 'contents' },
  header: {
    boxSizing: 'border-box',
    height: '3.5rem',
    flexShrink: 0,
    paddingInline: '1.25rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.75rem',
    minWidth: 0,
  },
  title: {
    margin: 0,
    fontSize: tokens.fontSizeBase500,
    lineHeight: tokens.lineHeightBase500,
    fontWeight: tokens.fontWeightSemibold,
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  subtitle: {
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    color: tokens.colorNeutralForeground2,
    whiteSpace: 'nowrap',
  },
})

export function Pane({ kind, label, children }: PaneProps) {
  const styles = useStyles()
  const heading = useSplitViewTitle()
  const [commandBar, setCommandBar] = useState<HTMLElement | null>(null)
  const [status, setStatus] = useState<HTMLElement | null>(null)
  return (
    <section
      className={mergeClasses(styles.pane, styles[kind])}
      data-pane={kind}
      aria-label={label}
    >
      {kind === 'list' && heading ? (
        <div className={styles.header}>
          <h1 className={styles.title}>{heading.title}</h1>
          {heading.subtitle ? <span className={styles.subtitle}>{heading.subtitle}</span> : null}
        </div>
      ) : null}
      <div ref={setCommandBar} className={styles.slot} />
      <PaneSlotsProvider value={{ commandBar, status }}>{children}</PaneSlotsProvider>
      <div ref={setStatus} className={styles.slot} />
    </section>
  )
}
