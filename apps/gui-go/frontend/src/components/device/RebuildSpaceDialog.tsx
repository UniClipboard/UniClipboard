import { Loader2 } from 'lucide-react'
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
import { useRebuildSpace } from '@/hooks/useRebuildSpace'

export default function RebuildSpaceDialog({
  onClose,
  onRebuildSucceeded,
}: {
  onClose: () => void
  onRebuildSucceeded?: () => void
}) {
  const { t } = useTranslation()
  const {
    confirmation: resetConfirmInput,
    setConfirmation: setResetConfirmInput,
    confirmed: resetConfirmTokenMatches,
    rebuilding: resetting,
    errorKey: resetErrorKey,
    rebuild,
  } = useRebuildSpace(onRebuildSucceeded)
  const closeResetModal = () => {
    if (resetting) return
    onClose()
  }

  const handleResetSubmit = async () => {
    if (await rebuild()) onClose()
  }

  return (
    <AlertDialog
      open
      onOpenChange={(open, eventDetails) => {
        if (open) return
        if (resetting) {
          eventDetails.cancel()
          return
        }
        closeResetModal()
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('devices.panel.danger.modal.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('devices.panel.danger.modal.warning')}</AlertDialogDescription>
        </AlertDialogHeader>

        <div className="space-y-2">
          <Label htmlFor="rebuild-space-confirm">
            {t('devices.panel.danger.modal.confirmPrompt')}
          </Label>
          <Input
            id="rebuild-space-confirm"
            type="text"
            value={resetConfirmInput}
            onChange={e => setResetConfirmInput(e.target.value)}
            placeholder={t('devices.panel.danger.modal.confirmPlaceholder')}
            disabled={resetting}
            autoComplete="off"
            spellCheck={false}
          />
        </div>

        {resetErrorKey && (
          <div
            role="alert"
            className="rounded-lg border border-destructive/20 bg-destructive/5 p-3"
          >
            <p className="text-ui-body font-medium text-destructive">{t(resetErrorKey)}</p>
          </div>
        )}

        <AlertDialogFooter>
          <Button variant="outline" onClick={closeResetModal} disabled={resetting}>
            {t('devices.panel.danger.modal.cancel')}
          </Button>
          <Button
            variant="destructive"
            onClick={handleResetSubmit}
            disabled={!resetConfirmTokenMatches || resetting}
          >
            {resetting ? (
              <>
                <Loader2 className="mr-2 size-4 animate-spin" />
                {t('devices.panel.danger.modal.resetting')}
              </>
            ) : (
              t('devices.panel.danger.modal.confirm')
            )}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
