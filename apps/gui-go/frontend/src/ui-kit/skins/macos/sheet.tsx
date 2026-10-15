import { Dialog } from '@base-ui/react/dialog'
import type { SheetProps } from '@/ui-kit/contract'
import { PushButton } from './parts'
import { usePortalContainer } from './portal'

/**
 * AppKit sheet (design MC3): drops from the top edge of the window over a scrim, keeps focus
 * inside, and Done is the default button. Base UI returns focus to the opener on close.
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
        <Dialog.Backdrop className="mac-sheet-backdrop" />
        <Dialog.Popup className="mac-sheet mac-sheet-wide">
          <div className="mac-sheet-header">
            <Dialog.Title className="mac-sheet-title">{title}</Dialog.Title>
            {header}
          </div>
          <div className="mac-sheet-body">{children}</div>
          <div className="mac-sheet-footer">
            {footer}
            <span className="mac-split-spacer" />
            <PushButton tint="primary" onClick={done.onClick}>
              {done.label}
            </PushButton>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
