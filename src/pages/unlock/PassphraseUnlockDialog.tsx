import { Eye, EyeOff, Loader2 } from 'lucide-react'
import { useState } from 'react'
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
import { contentUnlockErrorKey } from '@/lib/content-unlock-error'
import { commands } from '@/lib/ipc'

interface PassphraseUnlockDialogProps {
  open: boolean
  onClose: () => void
  onResetRequested: () => void
  onUnlocked: () => Promise<void>
}

export function PassphraseUnlockDialog({
  open,
  onClose,
  onResetRequested,
  onUnlocked,
}: PassphraseUnlockDialogProps) {
  const { t } = useTranslation()
  const [passphrase, setPassphrase] = useState('')
  const [showPassphrase, setShowPassphrase] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [errorKey, setErrorKey] = useState<string | null>(null)

  const close = () => {
    onClose()
    setPassphrase('')
    setShowPassphrase(false)
    setErrorKey(null)
  }

  const submit = async () => {
    if (passphrase.length === 0) {
      setErrorKey('unlock.errors.wrongPassphrase')
      return
    }

    setSubmitting(true)
    setErrorKey(null)
    try {
      await commands.unlockContent({ passphrase })
      close()
      await onUnlocked()
    } catch (error) {
      setErrorKey(contentUnlockErrorKey(error))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <AlertDialog
      open={open}
      onOpenChange={(nextOpen, eventDetails) => {
        if (nextOpen) return
        if (submitting) {
          eventDetails.cancel()
          return
        }
        close()
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('unlock.passphraseModal.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('unlock.passphraseModal.description')}</AlertDialogDescription>
        </AlertDialogHeader>

        <div className="space-y-2">
          <Label htmlFor="unlock-passphrase">{t('unlock.passphraseModal.passphraseLabel')}</Label>
          <div className="relative">
            <Input
              id="unlock-passphrase"
              type={showPassphrase ? 'text' : 'password'}
              value={passphrase}
              onChange={event => {
                setPassphrase(event.target.value)
                setErrorKey(null)
              }}
              onKeyDown={event => {
                if (event.key === 'Enter' && !submitting) void submit()
              }}
              disabled={submitting}
              placeholder={t('unlock.passphraseModal.passphrasePlaceholder')}
              className="pr-10"
              autoFocus
              aria-invalid={errorKey !== null}
            />
            <button
              type="button"
              onClick={() => setShowPassphrase(value => !value)}
              disabled={submitting}
              aria-label={t(
                showPassphrase ? 'unlock.passphraseModal.hide' : 'unlock.passphraseModal.show'
              )}
              className="absolute right-0 top-0 flex h-full items-center px-3 text-muted-foreground transition-colors hover:text-foreground disabled:opacity-50"
            >
              {showPassphrase ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
            </button>
          </div>
        </div>

        {errorKey && (
          <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3">
            <p className="text-ui-body font-medium text-destructive">{t(errorKey)}</p>
          </div>
        )}

        <p className="text-ui-caption text-muted-foreground">{t('unlock.passphraseModal.hint')}</p>

        <button
          type="button"
          onClick={() => {
            close()
            onResetRequested()
          }}
          disabled={submitting}
          className="self-start text-ui-body text-muted-foreground/70 underline-offset-4 transition-colors hover:text-muted-foreground hover:underline disabled:opacity-50"
        >
          {t('unlock.factoryReset.link')}
        </button>

        <AlertDialogFooter>
          <Button variant="outline" onClick={close} disabled={submitting}>
            {t('unlock.passphraseModal.cancel')}
          </Button>
          <Button onClick={() => void submit()} disabled={submitting}>
            {submitting ? (
              <>
                <Loader2 className="mr-2 size-4 animate-spin" />
                {t('unlock.passphraseModal.submitting')}
              </>
            ) : (
              t('unlock.passphraseModal.submit')
            )}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
