import { ProgressBar as FluentProgressBar } from '@fluentui/react-components'
import type { ProgressBarProps } from '@/ui-kit/contract'

/** WinUI ProgressBar: a thin track with an accent indicator; indeterminate without a value. */
export function ProgressBar({ label, value }: ProgressBarProps) {
  return (
    <FluentProgressBar
      aria-label={label}
      thickness="large"
      value={value === undefined ? undefined : Math.min(100, Math.max(0, value)) / 100}
    />
  )
}
