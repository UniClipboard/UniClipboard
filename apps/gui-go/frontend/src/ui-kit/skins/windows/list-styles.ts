import { makeStyles, tokens } from '@fluentui/react-components'

// WinUI ListViewItem: subtle fill on hover/selection, a 3x16 accent pill at the start of the
// selected row, Body text with a Caption subtitle.
export const useListStyles = makeStyles({
  list: { display: 'flex', flexDirection: 'column', gap: '0.125rem', padding: 0, margin: 0 },
  row: {
    position: 'relative',
    display: 'flex',
    alignItems: 'center',
    gap: '0.75rem',
    minHeight: '3.5rem',
    paddingInline: '0.75rem',
    borderRadius: tokens.borderRadiusMedium,
    listStyle: 'none',
    cursor: 'default',
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
    '&[aria-selected="true"]': { backgroundColor: 'var(--w11-selected)' },
    '&[aria-selected="true"]::before': {
      content: '""',
      position: 'absolute',
      insetInlineStart: 0,
      top: 'calc(50% - 0.5rem)',
      width: '0.1875rem',
      height: '1rem',
      borderRadius: '0.125rem',
      backgroundColor: tokens.colorBrandBackground,
    },
  },
  multi: { '&[aria-selected="true"]::before': { display: 'none' } },
  text: { display: 'flex', flexDirection: 'column', minWidth: 0, flexGrow: 1 },
  title: {
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  mono: { fontFamily: tokens.fontFamilyMonospace },
  subtitle: {
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
  trailing: { display: 'flex', alignItems: 'center', gap: '0.375rem', flexShrink: 0 },
})
