import { makeStyles, tokens } from '@fluentui/react-components'

export const useGroupedListStyles = makeStyles({
  menuArea: { flexGrow: 1, minHeight: 0, display: 'flex', flexDirection: 'column' },
  list: {
    flexGrow: 1,
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
    outline: 'none',
    // Keyboard focus is on the list; the ring marks the row the keyboard moves from.
    '&:focus-visible [data-cursor]': {
      outline: `${tokens.strokeWidthThick} solid ${tokens.colorStrokeFocus2}`,
    },
  },
  scroller: { flexGrow: 1, minHeight: 0 },
  // Sticky group headers must be opaque: the card layer is translucent.
  header: {
    boxSizing: 'border-box',
    minHeight: '2.25rem',
    paddingBlock: '0.5rem 0.25rem',
    paddingInline: '1rem',
    display: 'flex',
    alignItems: 'baseline',
    gap: '0.5rem',
    backgroundColor: 'var(--w11-card-solid)',
  },
  headerTitle: {
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    fontWeight: tokens.fontWeightSemibold,
  },
  headerDetail: {
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
  headerTrailing: {
    marginInlineStart: 'auto',
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground3,
  },
  cell: { paddingBlock: '0.0625rem', paddingInline: '0.25rem' },
  row: { userSelect: 'none' },
  // Bottom room so the last rows can scroll clear of a pane's action bar.
  footer: {
    paddingBlock: '0.75rem 5.5rem',
    paddingInline: '1rem',
    display: 'flex',
    justifyContent: 'center',
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
})
