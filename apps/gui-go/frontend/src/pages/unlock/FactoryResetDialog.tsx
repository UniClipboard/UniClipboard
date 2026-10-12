import { Loader2 } from 'lucide-react'
import type { RefObject } from 'react'
import { useTranslation } from 'react-i18next'
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useFactoryReset } from './useFactoryReset'

interface FactoryResetDialogProps {
  open: boolean
  onClose: () => void
  onResetSucceeded?: () => void
  /** Element that receives focus when the dialog closes, e.g. the control that opened it. */
  finalFocus?: RefObject<HTMLElement | null>
}

export function FactoryResetDialog({
  open,
  onClose,
  onResetSucceeded,
  finalFocus,
}: FactoryResetDialogProps) {
  const { t } = useTranslation()
  const {
    confirmation,
    setConfirmation,
    confirmed,
    resetting,
    restarting,
    errorKey,
    close,
    submit,
  } = useFactoryReset(onClose, onResetSucceeded)

  return (
    <AlertDialog
      open={open}
      onOpenChange={(nextOpen, eventDetails) => {
        if (nextOpen) return
        if (resetting || restarting) {
          eventDetails.cancel()
          return
        }
        close()
      }}
    >
      <AlertDialogContent finalFocus={finalFocus}>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('unlock.factoryReset.modal.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('unlock.factoryReset.modal.warning')}</AlertDialogDescription>
        </AlertDialogHeader>

        <div className="space-y-2">
          <Label htmlFor="factory-reset-confirm">
            {t('unlock.factoryReset.modal.confirmPrompt')}
          </Label>
          <Input
            id="factory-reset-confirm"
            type="text"
            value={confirmation}
            onChange={event => setConfirmation(event.target.value)}
            placeholder={t('unlock.factoryReset.modal.confirmPlaceholder')}
            disabled={resetting || restarting}
            autoFocus
            autoComplete="off"
            spellCheck={false}
          />
        </div>

        {errorKey && (
          <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3">
            <p className="text-ui-body font-medium text-destructive">{t(errorKey)}</p>
          </div>
        )}

        <AlertDialogFooter>
          <Button variant="outline" onClick={close} disabled={resetting || restarting}>
            {t('unlock.factoryReset.modal.cancel')}
          </Button>
          <Button
            variant="destructive"
            onClick={() => void submit()}
            disabled={!confirmed || resetting || restarting}
          >
            {resetting || restarting ? (
              <>
                <Loader2 className="mr-2 size-4 animate-spin" />
                {t(
                  restarting
                    ? 'unlock.factoryReset.modal.restarting'
                    : 'unlock.factoryReset.modal.resetting'
                )}
              </>
            ) : (
              t('unlock.factoryReset.modal.confirm')
            )}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
