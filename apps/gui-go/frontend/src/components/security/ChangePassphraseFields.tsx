import { Eye, EyeOff, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { DialogFooter } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { useChangePassphraseForm } from './useChangePassphraseForm'

/** A view whose owner decides how long the form and its pending operation live. */
export default function ChangePassphraseFields({
  model,
  onCancel,
}: {
  model: ReturnType<typeof useChangePassphraseForm>
  onCancel: () => void
}) {
  const { t } = useTranslation()
  const { passphrase, confirmation, visible, errorKey } = model.state
  const { submitting, mismatch } = model
  return (
    <form className="flex flex-col gap-4" onSubmit={model.handleSubmit}>
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
              model.dispatch({ type: 'edit', field: 'passphrase', value: event.target.value })
            }
            placeholder={t('passphraseChange.passphrasePlaceholder')}
            disabled={submitting}
            className="pr-10"
          />
          <button
            type="button"
            onClick={() => model.dispatch({ type: 'toggle_visibility' })}
            disabled={submitting}
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
            model.dispatch({ type: 'edit', field: 'confirmation', value: event.target.value })
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
