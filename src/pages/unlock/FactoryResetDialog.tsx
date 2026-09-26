import { Loader2 } from 'lucide-react'
import { useState, type RefObject } from 'react'
import { useTranslation } from 'react-i18next'
import { isFactoryResetError, resetSpace, type FactoryResetError } from '@/api/security'
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

const log = createLogger('factory-reset-dialog')
const CONFIRMATION_TOKEN = 'RESET'

interface FactoryResetDialogProps {
  open: boolean
  onClose: () => void
  onResetSucceeded?: () => void
  /** Element that receives focus when the dialog closes, e.g. the control that opened it. */
  finalFocus?: RefObject<HTMLElement | null>
}

function errorI18nKey(error: FactoryResetError): string {
  switch (error.code) {
    case 'KEY_MATERIAL_WIPE_FAILED':
      return 'unlock.factoryReset.errors.keyMaterialWipeFailed'
    case 'STORAGE_FAILED':
      return 'unlock.factoryReset.errors.storageFailed'
    case 'FACADE_UNAVAILABLE':
      return 'unlock.factoryReset.errors.facadeUnavailable'
    case 'INTERNAL':
      return 'unlock.factoryReset.errors.internal'
  }
}

export function FactoryResetDialog({
  open,
  onClose,
  onResetSucceeded,
  finalFocus,
}: FactoryResetDialogProps) {
  const { t } = useTranslation()
  const [confirmation, setConfirmation] = useState('')
  const [resetting, setResetting] = useState(false)
  const [errorKey, setErrorKey] = useState<string | null>(null)
  const confirmed = confirmation.trim() === CONFIRMATION_TOKEN

  const close = () => {
    if (resetting) return
    onClose()
    setConfirmation('')
    setErrorKey(null)
  }

  const submit = async () => {
    if (!confirmed || resetting) return
    setResetting(true)
    setErrorKey(null)
    try {
      await resetSpace()
      try {
        await refreshSetupState()
      } catch (error) {
        log.warn({ err: error }, 'Setup state refresh failed after reset')
      }
      onClose()
      setConfirmation('')
      onResetSucceeded?.()
    } catch (error) {
      if (isFactoryResetError(error)) {
        setErrorKey(errorI18nKey(error))
      } else {
        log.error({ err: error }, 'Unexpected factory reset error')
        setErrorKey('unlock.factoryReset.errors.internal')
      }
    } finally {
      setResetting(false)
    }
  }

  return (
    <AlertDialog
      open={open}
      onOpenChange={(nextOpen, eventDetails) => {
        if (nextOpen) return
        if (resetting) {
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
            disabled={resetting}
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
          <Button variant="outline" onClick={close} disabled={resetting}>
            {t('unlock.factoryReset.modal.cancel')}
          </Button>
          <Button
            variant="destructive"
            onClick={() => void submit()}
            disabled={!confirmed || resetting}
          >
            {resetting ? (
              <>
                <Loader2 className="mr-2 size-4 animate-spin" />
                {t('unlock.factoryReset.modal.resetting')}
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
