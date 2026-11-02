import type { ProgressBarProps, StatusTone } from '@/ui-kit/contract'
import { Icon } from './icons'
import { Spinner } from './spinner'

export function TransferIndicator({ tone, value, label }: ProgressBarProps & { tone: StatusTone }) {
  if (tone !== 'info') {
    return (
      <Icon
        name={tone === 'positive' ? 'checkCircle' : tone === 'critical' ? 'warning' : 'info'}
        size={18}
      />
    )
  }
  return (
    <span
      className="mac-transfer-ring"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value}
    >
      {value === undefined ? (
        <Spinner size={18} />
      ) : (
        <svg
          viewBox="0 0 24 24"
          aria-hidden="true"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
        >
          <circle cx="12" cy="12" r="9" opacity="0.16" />
          <circle
            cx="12"
            cy="12"
            r="9"
            pathLength="100"
            strokeDasharray="100"
            strokeDashoffset={100 - value}
            strokeLinecap="round"
            transform="rotate(-90 12 12)"
          />
        </svg>
      )}
    </span>
  )
}
