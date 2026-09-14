import { Eye, EyeOff, Loader2 } from 'lucide-react'
import { useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import { changeEncryptionPassphrase, getChangePassphraseErrorCode } from '@/api/daemon/encryption'
import { Button } from '@/components/ui/button'
import { DialogFooter } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { createLogger } from '@/lib/logger'

const log = createLogger('change-passphrase')

interface ChangePassphraseState {
  passphrase: string
  confirmation: string
  visible: boolean
  submitting: boolean
  errorKey: string | null
}

type ChangePassphraseAction =
  | { type: 'edit'; field: 'passphrase' | 'confirmation'; value: string }
  | { type: 'toggle_visibility' }
  | { type: 'submit' }
  | { type: 'failed'; errorKey: string }

const initialState: ChangePassphraseState = {
  passphrase: '',
  confirmation: '',
  visible: false,
  submitting: false,
  errorKey: null,
}

function reducer(
  state: ChangePassphraseState,
  action: ChangePassphraseAction
): ChangePassphraseState {
  switch (action.type) {
    case 'edit':
      return { ...state, [action.field]: action.value, errorKey: null }
    case 'toggle_visibility':
      return { ...state, visible: !state.visible }
    case 'submit':
      return { ...state, submitting: true, errorKey: null }
    case 'failed':
      return { ...state, submitting: false, errorKey: action.errorKey }
  }
}

function errorTranslationKey(error: unknown): string {
  switch (getChangePassphraseErrorCode(error)) {
    case 'PASSPHRASE_MISMATCH':
      return 'passphraseChange.errors.mismatch'
    case 'MULTIPLE_DEVICES':
      return 'passphraseChange.errors.multipleDevices'
    case 'SPACE_LOCKED':
      return 'passphraseChange.errors.locked'
    case 'MEMBERSHIP_RECOVERY_REQUIRED':
    case 'RECOVERY_REQUIRED':
      return 'passphraseChange.errors.recovery'
    case 'runtime_unavailable':
      return 'passphraseChange.errors.unavailable'
    default:
      return 'passphraseChange.errors.internal'
  }
}

export default function ChangePassphraseForm({
  onCancel,
  onChanged,
}: {
  onCancel: () => void
  onChanged: () => void
}) {
  const { t } = useTranslation()
  const [state, dispatch] = useReducer(reducer, initialState)
  const { passphrase, confirmation, visible, submitting, errorKey } = state
  const mismatch = confirmation.length > 0 && passphrase !== confirmation

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!passphrase || !confirmation || submitting) return
    if (passphrase !== confirmation) {
      dispatch({ type: 'failed', errorKey: 'passphraseChange.errors.mismatch' })
      return
    }

    dispatch({ type: 'submit' })
    try {
      await changeEncryptionPassphrase(passphrase, confirmation)
      log.info({ event: 'passphrase_change_succeeded' }, 'space passphrase changed')
      onChanged()
    } catch (error) {
      log.warn(
        { error_kind: getChangePassphraseErrorCode(error) ?? 'unknown' },
        'space passphrase change failed'
      )
      dispatch({ type: 'failed', errorKey: errorTranslationKey(error) })
    }
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
      <div className="flex flex-col gap-2">
        <Label htmlFor="new-space-passphrase">{t('passphraseChange.passphraseLabel')}</Label>
        <div className="relative">
          <Input
            id="new-space-passphrase"
            type={visible ? 'text' : 'password'}
            autoComplete="new-password"
            autoFocus
            value={passphrase}
            onChange={event =>
              dispatch({ type: 'edit', field: 'passphrase', value: event.target.value })
            }
            placeholder={t('passphraseChange.passphrasePlaceholder')}
            disabled={submitting}
            className="pr-10"
          />
          <button
            type="button"
            onClick={() => dispatch({ type: 'toggle_visibility' })}
            aria-label={t(visible ? 'passphraseChange.hide' : 'passphraseChange.show')}
            className="absolute top-0 right-0 flex h-full items-center px-3 text-muted-foreground transition-colors hover:text-foreground"
          >
            {visible ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
          </button>
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <Label htmlFor="confirm-space-passphrase">{t('passphraseChange.confirmationLabel')}</Label>
        <Input
          id="confirm-space-passphrase"
          type={visible ? 'text' : 'password'}
          autoComplete="new-password"
          value={confirmation}
          onChange={event =>
            dispatch({ type: 'edit', field: 'confirmation', value: event.target.value })
          }
          placeholder={t('passphraseChange.confirmationPlaceholder')}
          disabled={submitting}
          aria-invalid={mismatch || undefined}
        />
      </div>

      {(errorKey || mismatch) && (
        <p role="alert" className="text-ui-body font-medium text-destructive">
          {t(errorKey ?? 'passphraseChange.errors.mismatch')}
        </p>
      )}

      <DialogFooter className="mt-2">
        <Button type="button" variant="outline" onClick={onCancel} disabled={submitting}>
          {t('passphraseChange.cancel')}
        </Button>
        <Button type="submit" disabled={submitting || !passphrase || !confirmation || mismatch}>
          {submitting && <Loader2 className="size-4 animate-spin" />}
          {t(submitting ? 'passphraseChange.submitting' : 'passphraseChange.submit')}
        </Button>
      </DialogFooter>
    </form>
  )
}
