import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isInvitationCodeComplete,
  normalizeInvitationCode,
} from '@/components/invitation-code-utils'
import { InvitationCodeInput } from '@/components/InvitationCodeInput'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { clearMutationError, createSpace, joinSpace } from '@/store/spacesSlice'

interface AddSpaceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

type AddSpaceMode = 'join' | 'create'

export default function AddSpaceDialog(props: AddSpaceDialogProps) {
  return <AddSpaceDialogInner key={props.open ? 'open' : 'closed'} {...props} />
}

function AddSpaceDialogInner({ open, onOpenChange }: AddSpaceDialogProps) {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const mutationError = useAppSelector(state => state.spaces.mutationError)
  const [mode, setMode] = useState<AddSpaceMode>('join')
  const [code, setCode] = useState('')
  const [passphrase, setPassphrase] = useState('')
  const [passphraseConfirm, setPassphraseConfirm] = useState('')
  const [deviceName, setDeviceName] = useState('')
  const [submitting, setSubmitting] = useState(false)

  useEffect(() => {
    dispatch(clearMutationError())
  }, [dispatch])

  const passphrasesMismatch =
    mode === 'create' && passphraseConfirm.length > 0 && passphrase !== passphraseConfirm
  const codeComplete = isInvitationCodeComplete(code)
  const canSubmit =
    !submitting &&
    passphrase.length > 0 &&
    (mode === 'join' ? codeComplete : passphraseConfirm.length > 0 && !passphrasesMismatch)

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!canSubmit) return
    setSubmitting(true)
    const normalizedDeviceName = deviceName.trim() || null
    try {
      if (mode === 'join') {
        await dispatch(
          joinSpace({
            code: normalizeInvitationCode(code),
            passphrase,
            deviceName: normalizedDeviceName,
          })
        ).unwrap()
      } else {
        await dispatch(
          createSpace({
            passphrase,
            passphraseConfirm,
            deviceName: normalizedDeviceName,
          })
        ).unwrap()
      }
      onOpenChange(false)
    } catch {
      // The thunk refreshes the authoritative list and stores a localized error key.
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t('spaces.dialog.title')}</DialogTitle>
          <DialogDescription>{t('spaces.dialog.subtitle')}</DialogDescription>
        </DialogHeader>
        <div className="grid grid-cols-2 gap-2" role="group" aria-label={t('spaces.dialog.title')}>
          <Button
            type="button"
            variant={mode === 'join' ? 'default' : 'outline'}
            aria-pressed={mode === 'join'}
            disabled={submitting}
            onClick={() => setMode('join')}
          >
            {t('spaces.dialog.joinMode')}
          </Button>
          <Button
            type="button"
            variant={mode === 'create' ? 'default' : 'outline'}
            aria-pressed={mode === 'create'}
            disabled={submitting}
            onClick={() => setMode('create')}
          >
            {t('spaces.dialog.createMode')}
          </Button>
        </div>

        <form className="space-y-4" onSubmit={event => void handleSubmit(event)}>
          {mode === 'join' ? (
            <div className="space-y-2">
              <Label htmlFor="add-space-code">{t('spaces.dialog.code')}</Label>
              <InvitationCodeInput
                id="add-space-code"
                value={code}
                onChange={setCode}
                autoComplete="one-time-code"
                autoFocus
              />
            </div>
          ) : null}

          <div className="space-y-2">
            <Label htmlFor="add-space-passphrase">{t('spaces.dialog.passphrase')}</Label>
            <Input
              id="add-space-passphrase"
              type="password"
              value={passphrase}
              onChange={event => setPassphrase(event.target.value)}
              autoComplete="current-password"
            />
          </div>

          {mode === 'create' ? (
            <div className="space-y-2">
              <Label htmlFor="add-space-passphrase-confirm">
                {t('spaces.dialog.confirmPassphrase')}
              </Label>
              <Input
                id="add-space-passphrase-confirm"
                type="password"
                value={passphraseConfirm}
                onChange={event => setPassphraseConfirm(event.target.value)}
                aria-invalid={passphrasesMismatch}
                aria-describedby={passphrasesMismatch ? 'add-space-passphrase-error' : undefined}
                autoComplete="new-password"
              />
              {passphrasesMismatch ? (
                <p id="add-space-passphrase-error" className="text-xs text-destructive">
                  {t('spaces.dialog.passphraseMismatch')}
                </p>
              ) : null}
            </div>
          ) : null}

          <div className="space-y-2">
            <Label htmlFor="add-space-device-name">{t('spaces.dialog.deviceName')}</Label>
            <Input
              id="add-space-device-name"
              value={deviceName}
              onChange={event => setDeviceName(event.target.value)}
              autoComplete="off"
            />
          </div>

          {mutationError ? (
            <p role="alert" className="text-sm font-medium text-destructive">
              {t(mutationError)}
            </p>
          ) : null}

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={submitting}
              onClick={() => onOpenChange(false)}
            >
              {t('spaces.actions.cancel')}
            </Button>
            <Button type="submit" disabled={!canSubmit}>
              {submitting
                ? t(mode === 'join' ? 'spaces.dialog.joining' : 'spaces.dialog.creating')
                : t(mode === 'join' ? 'spaces.actions.join' : 'spaces.actions.create')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
