import { PreviewCard } from '@base-ui/react/preview-card'
import type { TransferStatusProps } from '@/ui-kit/contract'
import { usePortalContainer } from './portal'
import { TransferIndicator } from './transfer-indicator'
import { TransferItem } from './transfer-item'

/** Platform-owned transfer inspector; Base UI owns hover, focus, Escape and positioning. */
export function TransferStatus({
  'data-testid': testId,
  label,
  title,
  status,
  summary,
  tone,
  value,
  disabled,
  onRetry,
  items,
}: TransferStatusProps) {
  const container = usePortalContainer()
  return (
    <PreviewCard.Root>
      <PreviewCard.Trigger
        render={<button type="button" />}
        delay={180}
        closeDelay={180}
        className="adw-transfer-trigger"
        data-tone={tone}
        data-testid={testId}
        aria-label={label}
        aria-disabled={disabled || undefined}
        onClick={disabled ? undefined : onRetry}
      >
        <TransferIndicator tone={tone} value={value} label={label} />
      </PreviewCard.Trigger>
      <PreviewCard.Portal container={container}>
        <PreviewCard.Positioner className="adw-positioner" sideOffset={8} align="end">
          <PreviewCard.Popup
            className="adw-menu adw-transfer"
            data-testid="delivery-transfer-details"
            data-transfer-skin="linux"
            aria-label={title}
          >
            <header className="adw-transfer-header">
              <div className="adw-transfer-heading">
                <span className="adw-transfer-name">{title}</span>
                <span className="adw-transfer-status" data-tone={tone}>
                  {status}
                </span>
              </div>
              {summary && <span className="adw-transfer-summary">{summary}</span>}
            </header>
            {items.length > 0 && (
              <ul className="adw-transfer-files" aria-label={title} tabIndex={0}>
                {items.map(item => (
                  <TransferItem key={item.id} item={item} />
                ))}
              </ul>
            )}
          </PreviewCard.Popup>
        </PreviewCard.Positioner>
      </PreviewCard.Portal>
    </PreviewCard.Root>
  )
}
