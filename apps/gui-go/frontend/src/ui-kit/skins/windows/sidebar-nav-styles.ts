import { makeStyles, tokens } from '@fluentui/react-components'

export const useSidebarNavStyles = makeStyles({
  root: { flexGrow: 1, display: 'flex', flexDirection: 'column', minHeight: 0 },
  nav: { backgroundColor: 'transparent', width: '100%', flexGrow: 1 },
  // Fluent Nav paints items with its drawer background; WinUI pane items are transparent at rest.
  // Board WA2 / C2: 36 px items, 14 px text, the selected item's icon in the accent colour.
  item: {
    justifyContent: 'flex-start',
    gap: '0.875rem',
    minHeight: '2.25rem',
    paddingBlock: 0,
    alignItems: 'center',
    '& > span:first-child': { display: 'inline-flex', alignItems: 'center' },
    fontSize: tokens.fontSizeBase300,
    backgroundColor: 'transparent',
    fontWeight: tokens.fontWeightRegular,
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
    ':active': { backgroundColor: tokens.colorSubtleBackgroundPressed },
    '&[aria-current="page"]': {
      backgroundColor: 'var(--w11-selected)',
      fontWeight: tokens.fontWeightSemibold,
    },
    '&[aria-current="page"] svg': { color: tokens.colorBrandForeground1 },
  },
  label: {
    flexGrow: 1,
    minWidth: 0,
    textAlign: 'start',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  count: { color: tokens.colorNeutralForeground2, fontSize: tokens.fontSizeBase200 },
  hint: { color: tokens.colorNeutralForeground3, fontSize: tokens.fontSizeBase200 },
  // Swatches and status dots sit centred in the 20px icon box.
  iconBox: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '1.25rem',
    height: '1.25rem',
  },
  swatch: {
    display: 'inline-block',
    width: '0.75rem',
    height: '0.75rem',
    borderRadius: '0.1875rem',
  },
  heading: {
    display: 'flex',
    alignItems: 'center',
    gap: '0.25rem',
    marginTop: '0.625rem',
    paddingInlineEnd: '0.25rem',
  },
  headingTitle: {
    flexGrow: 1,
    minWidth: 0,
    fontSize: tokens.fontSizeBase200,
    fontWeight: tokens.fontWeightSemibold,
    color: tokens.colorNeutralForeground2,
  },
  more: {
    justifyContent: 'flex-start',
    minHeight: '2rem',
    paddingInlineStart: '2.875rem',
    fontWeight: tokens.fontWeightRegular,
    color: tokens.colorNeutralForeground2,
  },
  placeholder: {
    display: 'flex',
    flexDirection: 'column',
    gap: '0.5rem',
    paddingInline: '0.5rem',
    paddingBlock: '0.25rem',
  },
  placeholderText: {
    padding: '0.5rem 0.75rem',
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
  // NavigationView FooterMenuItems: pinned under the scrolling items.
  footer: {
    marginTop: 'auto',
    paddingTop: '0.5rem',
    display: 'flex',
    flexDirection: 'column',
  },
})
