import { makeStyles, tokens } from '@fluentui/react-components'
import type { ButtonVariant, ControlSize } from '@/ui-kit/contract'

export const appearance = {
  primary: 'primary',
  standard: 'secondary',
  subtle: 'subtle',
  // Win11 has no destructive push button (WinUI keeps destructive commands standard; the text carries the warning).
  destructive: 'secondary',
} as const satisfies Record<ButtonVariant, string>

// Fluent buttons stop at `large` (40px), which is also the largest WinUI control height.
export const fluentSize = {
  small: 'small',
  medium: 'medium',
  large: 'large',
  xlarge: 'large',
} as const satisfies Record<ControlSize, string>

// WinUI standard controls have a darker bottom edge ("elevation border") that Fluent web lacks.
// Pressed buttons lose it, as in the WinUI ButtonBorderBrushPressed resource.
export const useElevationStyles = makeStyles({
  // Win11 button text is Body (regular); Fluent web buttons default to semibold.
  text: { fontWeight: tokens.fontWeightRegular },
  raised: {
    borderBottomColor: 'var(--w11-stroke-elevation)',
    ':hover': { borderBottomColor: 'var(--w11-stroke-elevation)' },
    ':hover:active': { borderBottomColor: 'transparent' },
  },
})
