import { Spinner } from '@fluentui/react-components'
import { CheckmarkCircleRegular, InfoRegular, WarningRegular } from '@fluentui/react-icons'
import type { ProgressBarProps, StatusTone } from '@/ui-kit/contract'
import { useTransferStyles } from './transfer-styles'

export function TransferIndicator({ tone, value, label }: ProgressBarProps & { tone: StatusTone }) {
  const styles = useTransferStyles()
  if (tone !== 'info') {
    const Icon =
      tone === 'positive'
        ? CheckmarkCircleRegular
        : tone === 'critical'
          ? WarningRegular
          : InfoRegular
    return <Icon aria-hidden="true" />
  }
  return (
    <span
      className={styles.ring}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value}
    >
      {value === undefined ? (
        <Spinner size="extra-tiny" aria-hidden="true" />
      ) : (
        <svg
          viewBox="0 0 24 24"
          aria-hidden="true"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
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
