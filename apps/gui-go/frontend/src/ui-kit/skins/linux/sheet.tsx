import { Dialog } from '@base-ui/react/dialog'
import type { SheetProps } from '@/ui-kit/contract'
import { PushButton } from './parts'
import { usePortalContainer } from './portal'

/**
 * Modal dialog centred over a scrim, keeps focus inside, and Done is the default button. Base UI returns focus to the opener on close.
 */
export function Sheet({
  open,
  onOpenChange,
  title,
  header,
  children,
  footer,
  done,
  onClosed,
}: SheetProps) {
  const container = usePortalContainer()
  return (
    <Dialog.Root
      open={open}
      onOpenChange={next => onOpenChange(next)}
      onOpenChangeComplete={next => {
        if (!next) onClosed?.()
      }}
    >
      <Dialog.Portal container={container}>
        <Dialog.Backdrop className="adw-sheet-backdrop" />
        <Dialog.Popup className="adw-sheet adw-sheet-wide">
          <div className="adw-sheet-header">
            <Dialog.Title className="adw-sheet-title">{title}</Dialog.Title>
            {header}
          </div>
          <div className="adw-sheet-body">{children}</div>
          <div className="adw-sheet-footer">
            {footer}
            <span className="adw-split-spacer" />
            <PushButton tint="primary" onClick={done.onClick}>
              {done.label}
            </PushButton>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
