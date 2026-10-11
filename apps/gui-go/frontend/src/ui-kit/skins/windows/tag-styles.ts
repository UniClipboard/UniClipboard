import { makeStyles, tokens } from '@fluentui/react-components'
import type { TagPaletteColor } from '@/ui-kit/contract'

const colored = (color: TagPaletteColor | 'neutral') => ({
  backgroundColor: `var(--w11-tag-${color}-bg)`,
  color: `var(--w11-tag-${color}-ink)`,
})

// A chip is the tag text plus, when removable, a separate remove button inside the same shape.
export const useTagStyles = makeStyles({
  chip: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '0.125rem',
    height: '1.25rem',
    paddingInline: '0.375rem',
    borderRadius: tokens.borderRadiusMedium,
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    whiteSpace: 'nowrap',
  },
  name: { fontWeight: tokens.fontWeightSemibold },
  detail: { marginInlineStart: '0.25rem', fontWeight: tokens.fontWeightRegular, opacity: 0.85 },
  // Board WC2: dashed outline for a tag only some of the selected items carry.
  partial: { outline: `${tokens.strokeWidthThin} dashed currentColor`, outlineOffset: '-1px' },
  action: {
    display: 'inline-flex',
    alignItems: 'center',
    padding: 0,
    border: 'none',
    background: 'transparent',
    color: 'inherit',
    font: 'inherit',
    cursor: 'default',
    ':focus-visible': { outline: `${tokens.strokeWidthThick} solid ${tokens.colorStrokeFocus2}` },
  },
  orange: colored('orange'),
  blue: colored('blue'),
  gray: colored('gray'),
  purple: colored('purple'),
  green: colored('green'),
  neutral: colored('neutral'),
  token: {
    height: '1.5rem',
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
    borderBottomColor: 'var(--w11-stroke-elevation)',
    backgroundColor: tokens.colorNeutralBackground1,
  },
  field: { color: tokens.colorNeutralForeground3, marginInlineEnd: '0.25rem' },
  value: { fontWeight: tokens.fontWeightSemibold },
})
