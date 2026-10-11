import { makeStyles, tokens } from '@fluentui/react-components'
import type { TagColor } from '@/ui-kit/contract'

const colored = (color: TagColor) => ({
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
  deploy: colored('deploy'),
  docker: colored('docker'),
  git: colored('git'),
  design: colored('design'),
  finance: colored('finance'),
  work: colored('work'),
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
