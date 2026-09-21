import { ShieldAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { UpdateMetadata } from '@/api/updater'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { ReleaseNotes } from '@/components/update/ReleaseNotes'

interface UpdateConfirmationDialogProps {
  open: boolean
  update: UpdateMetadata | null
  confirming: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => Promise<void>
  presentation?: 'dialog' | 'window'
}

export function UpdateConfirmationDialog({
  open,
  update,
  confirming,
  onOpenChange,
  onConfirm,
  presentation = 'dialog',
}: UpdateConfirmationDialogProps) {
  const { t } = useTranslation()
  const confirmation = update?.confirmation
  const canConfirm = confirmation?.status === 'pending'

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent
        className={
          presentation === 'window'
            ? 'inset-0 top-0 left-0 flex h-screen w-screen max-w-none translate-x-0 translate-y-0 flex-col gap-4 rounded-none border-0 p-6 data-[size=default]:max-w-none data-[size=default]:sm:max-w-none'
            : 'w-[calc(100%-2rem)] data-[size=default]:max-w-lg data-[size=default]:sm:max-w-lg'
        }
      >
        <AlertDialogHeader
          className={
            presentation === 'window'
              ? 'min-h-0 flex-1 grid-cols-[auto_1fr] grid-rows-[auto_1fr] place-items-start gap-x-4 gap-y-3 text-left'
              : 'grid-cols-[auto_1fr] grid-rows-[auto_auto] place-items-start gap-x-4 gap-y-3 text-left'
          }
        >
          <div
            data-slot="alert-dialog-media"
            className="col-start-1 row-start-1 inline-flex size-10 items-center justify-center self-start rounded-md bg-amber-500/10 text-amber-700 dark:text-amber-300"
          >
            <ShieldAlert className="size-5" />
          </div>
          <AlertDialogTitle className="col-start-2 row-start-1 self-center">
            {t('update.confirmation.title')}
          </AlertDialogTitle>
          <AlertDialogDescription className="col-span-2 row-start-2 w-full" asChild>
            <div className={presentation === 'window' ? 'min-h-0 flex-1' : undefined}>
              {canConfirm ? (
                <div
                  className={
                    presentation === 'window'
                      ? 'scrollbar-thin h-full overflow-auto rounded-md border border-border/60 bg-muted/30 px-4 py-3 text-sm text-foreground'
                      : 'scrollbar-thin max-h-64 overflow-auto rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-foreground'
                  }
                >
                  <ReleaseNotes
                    content={confirmation.description}
                    fallback={t('update.confirmation.unavailable')}
                  />
                </div>
              ) : (
                <p className="text-sm font-medium text-foreground">
                  {t('update.confirmation.unavailable')}
                </p>
              )}
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter className={presentation === 'window' ? 'mt-auto' : undefined}>
          <AlertDialogCancel disabled={confirming}>
            {t('update.confirmation.cancel')}
          </AlertDialogCancel>
          {canConfirm && (
            <AlertDialogAction
              disabled={confirming}
              onClick={event => {
                event.preventDefault()
                void onConfirm()
              }}
            >
              {confirming
                ? t('update.confirmation.confirming')
                : t('update.confirmation.acknowledge')}
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
