import { Button, Popover, PopoverSurface, PopoverTrigger } from '@fluentui/react-components'
import { useRef, useState } from 'react'
import type { TransferStatusProps } from '@/ui-kit/contract'
import { useTransferStyles } from './transfer-styles'
import { TransferIndicator } from './TransferIndicator'
import { TransferItem } from './TransferItem'

/** Fluent owns the flyout and hover delay; focus also exposes the read-only details. */
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
  const styles = useTransferStyles()
  const [open, setOpen] = useState(false)
  const trigger = useRef<HTMLButtonElement>(null)
  const surface = useRef<HTMLDivElement>(null)
  return (
    <Popover
      openOnHover
      mouseLeaveDelay={180}
      open={open}
      onOpenChange={(_, data) => setOpen(data.open)}
      positioning="below-end"
      unstable_disableAutoFocus
    >
      <PopoverTrigger disableButtonEnhancement>
        <Button
          ref={trigger}
          type="button"
          appearance="subtle"
          shape="square"
          size="small"
          className={styles.trigger}
          data-tone={tone}
          data-testid={testId}
          aria-label={label}
          disabledFocusable={disabled}
          onFocus={() => setOpen(true)}
          onBlur={event => {
            if (!surface.current?.contains(event.relatedTarget)) setOpen(false)
          }}
          onClick={disabled ? undefined : onRetry}
          icon={<TransferIndicator tone={tone} value={value} label={label} />}
        />
      </PopoverTrigger>
      <PopoverSurface
        ref={surface}
        className={styles.surface}
        data-testid="delivery-transfer-details"
        data-transfer-skin="windows"
        aria-label={title}
        onBlur={event => {
          if (
            !surface.current?.contains(event.relatedTarget) &&
            !trigger.current?.contains(event.relatedTarget)
          )
            setOpen(false)
        }}
      >
        <header className={styles.header}>
          <div className={styles.heading}>
            <span className={styles.name}>{title}</span>
            <span className={styles.status} data-tone={tone}>
              {status}
            </span>
          </div>
          {summary && <span className={styles.summary}>{summary}</span>}
        </header>
        {items.length > 0 && (
          <ul className={styles.files} aria-label={title} tabIndex={0}>
            {items.map(item => (
              <TransferItem key={item.id} item={item} />
            ))}
          </ul>
        )}
      </PopoverSurface>
    </Popover>
  )
}
