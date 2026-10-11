import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
} from '@fluentui/react-components'
import type { SheetProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract (ContentDialog); the Win11 styling belongs to the
// Windows History work. Fluent's Dialog traps focus and restores it to the opener.
export function Sheet({ open, onOpenChange, title, header, children, footer, done }: SheetProps) {
  return (
    <Dialog open={open} onOpenChange={(_, data) => onOpenChange(data.open)}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>{title}</DialogTitle>
          <DialogContent>
            {header}
            {children}
          </DialogContent>
          <DialogActions>
            {footer}
            <Button appearance="primary" onClick={done.onClick}>
              {done.label}
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  )
}
