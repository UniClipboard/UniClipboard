import { Loader2 } from 'lucide-react'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { cn } from '@/lib/utils'
import type { SettingAlertProps } from './types'

export function SettingAlert({
  open,
  onOpenChange,
  title,
  description,
  children,
  confirmLabel,
  cancelLabel,
  destructive,
  onConfirm,
  onCancel,
  confirmDisabled,
  busyLabel,
  media,
  busyReplacesActions,
  cancelHint,
  confirmHint,
}: SettingAlertProps) {
  const busy = busyLabel !== undefined
  return (
    <AlertDialog
      open={open}
      onOpenChange={(next, eventDetails) => {
        // Running work cannot be dismissed (Escape, overlay, programmatic).
        if (!next && busy) {
          eventDetails.cancel()
          return
        }
        onOpenChange(next)
      }}
    >
      <AlertDialogContent className="bg-card text-card-foreground">
        <AlertDialogHeader>
          {media && (
            <AlertDialogMedia className="bg-amber-500/10 text-amber-600 dark:text-amber-400">
              {media}
            </AlertDialogMedia>
          )}
          <AlertDialogTitle>{title}</AlertDialogTitle>
          {description && <AlertDialogDescription>{description}</AlertDialogDescription>}
        </AlertDialogHeader>
        {children}
        {busy && busyReplacesActions ? (
          <AlertDialogFooter>
            <div className="flex w-full items-center justify-center gap-2 text-ui-body text-muted-foreground">
              <Loader2 className="size-4 animate-spin" />
              {busyLabel}
            </div>
          </AlertDialogFooter>
        ) : (
          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy} onClick={onCancel}>
              {cancelHint ? (
                <span className="flex items-center gap-1.5">
                  {cancelLabel}
                  {cancelHint}
                </span>
              ) : (
                cancelLabel
              )}
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={confirmDisabled || busy}
              className={cn(
                destructive && 'bg-destructive text-destructive-foreground hover:bg-destructive/90'
              )}
              onClick={event => {
                // The alert stays open until the caller closes it, or the confirmation is synchronous.
                event.preventDefault()
                const result = onConfirm()
                if (!(result instanceof Promise)) onOpenChange(false)
              }}
            >
              {busy ? (
                busyLabel
              ) : confirmHint ? (
                <span className="flex items-center gap-1.5">
                  {confirmLabel}
                  {confirmHint}
                </span>
              ) : (
                confirmLabel
              )}
            </AlertDialogAction>
          </AlertDialogFooter>
        )}
      </AlertDialogContent>
    </AlertDialog>
  )
}
