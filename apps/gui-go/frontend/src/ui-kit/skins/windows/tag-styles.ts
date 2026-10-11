import { makeStyles, tokens } from '@fluentui/react-components'
import type { TagColor } from '@/ui-kit/contract'

const colored = (color: TagColor) => ({
  backgroundColor: `var(--w11-tag-${color}-bg)`,
  color: `var(--w11-tag-${color}-ink)`,
  border: 'none',
})

export const useTagStyles = makeStyles({
  base: {
    fontWeight: tokens.fontWeightSemibold,
    borderRadius: tokens.borderRadiusMedium,
  },
  deploy: colored('deploy'),
  docker: colored('docker'),
  git: colored('git'),
  design: colored('design'),
  finance: colored('finance'),
  work: colored('work'),
  neutral: colored('neutral'),
  token: { borderRadius: tokens.borderRadiusMedium },
  field: { color: tokens.colorNeutralForeground3, marginInlineEnd: '0.25rem' },
  value: { fontWeight: tokens.fontWeightSemibold },
})
