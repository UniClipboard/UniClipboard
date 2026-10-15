import { makeStyles, tokens } from '@fluentui/react-components'
import type { StatusTone } from '@/ui-kit/contract'

const tone = (name: StatusTone) => `var(--w11-status-${name})`

// Status colours of WinUI (SystemFillColorSuccess/Attention/Caution/Critical/Neutral): a dot and its text.
export const useStatusStyles = makeStyles({
  dot: {
    display: 'inline-block',
    flexShrink: 0,
    width: '0.5rem',
    height: '0.5rem',
    borderRadius: '50%',
  },
  dotPositive: { backgroundColor: tone('positive') },
  dotInfo: { backgroundColor: tone('info') },
  dotNeutral: { backgroundColor: tone('neutral') },
  dotWarning: { backgroundColor: tone('warning') },
  dotCritical: { backgroundColor: tone('critical') },
  textPositive: { color: tone('positive') },
  textInfo: { color: tone('info') },
  textNeutral: { color: tokens.colorNeutralForeground2 },
  textWarning: { color: tone('warning') },
  textCritical: { color: tone('critical') },
})

export const dotClass = {
  positive: 'dotPositive',
  info: 'dotInfo',
  neutral: 'dotNeutral',
  warning: 'dotWarning',
  critical: 'dotCritical',
} as const satisfies Record<StatusTone, string>

export const textClass = {
  positive: 'textPositive',
  info: 'textInfo',
  neutral: 'textNeutral',
  warning: 'textWarning',
  critical: 'textCritical',
} as const satisfies Record<StatusTone, string>
