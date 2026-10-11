import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
} from '@fluentui/react-components'
import type { QuickLookProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract; styling belongs to the Windows History work.
export function QuickLook({
  open,
  onOpenChange,
  title,
  closeLabel,
  src,
  alt,
  previous,
  next,
  primary,
  actions,
}: QuickLookProps) {
  return (
    <Dialog open={open} onOpenChange={(_, data) => onOpenChange(data.open)}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>{title}</DialogTitle>
          <DialogContent>
            {src ? <img src={src} alt={alt} style={{ maxWidth: '100%' }} /> : null}
          </DialogContent>
          <DialogActions>
            {actions}
            {previous ? <Button onClick={previous.onClick}>{previous.label}</Button> : null}
            {next ? <Button onClick={next.onClick}>{next.label}</Button> : null}
            {primary ? (
              <Button appearance="primary" onClick={primary.onClick}>
                {primary.label}
              </Button>
            ) : null}
            <Button onClick={() => onOpenChange(false)}>{closeLabel}</Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  )
}
