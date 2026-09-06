import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { resetSetup } from '@/api/daemon/setupV2'
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
import { createLogger } from '@/lib/logger'
import { refreshSetupState } from '@/store/setupRealtimeStore'

const log = createLogger('rebuild-space-dialog')
const REBUILD_CONFIRMATION_TOKEN = 'RESET'

export default function RebuildSpaceDialog({
  onClose,
  onRebuildSucceeded,
}: {
  onClose: () => void
  onRebuildSucceeded?: () => void
}) {
  const { t } = useTranslation()
  const [resetConfirmInput, setResetConfirmInput] = useState('')
  const [resetting, setResetting] = useState(false)
  const [resetErrorKey, setResetErrorKey] = useState<string | null>(null)
  const closeResetModal = () => {
    if (resetting) return
    onClose()
  }

  const resetConfirmTokenMatches =
    resetConfirmInput.trim().toUpperCase() === REBUILD_CONFIRMATION_TOKEN

  const handleResetSubmit = async () => {
    if (!resetConfirmTokenMatches || resetting) return
    setResetting(true)
    setResetErrorKey(null)
    try {
      await resetSetup()
      try {
        await refreshSetupState()
      } catch (refreshErr) {
        log.warn({ err: refreshErr }, 'Setup state refresh failed after space rebuild')
      }
      onRebuildSucceeded?.()
      onClose()
    } catch (error) {
      log.error({ err: error }, 'Space rebuild failed')
      setResetErrorKey('devices.panel.danger.failed')
    } finally {
      setResetting(false)
    }
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
          <Label htmlFor="rebuild-space-confirm" className="text-sm">
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
            <p className="text-sm font-medium text-destructive">{t(resetErrorKey)}</p>
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
