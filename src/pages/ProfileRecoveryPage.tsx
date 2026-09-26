import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AppStateShell } from '@/components/app/AppStateShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { contentUnlockErrorKey } from '@/lib/content-unlock-error'
import { commands } from '@/lib/ipc'
import type { ProfileRecoveryResponse } from '@/lib/ipc-bindings.generated'
import { createLogger } from '@/lib/logger'
import { ensureSetupRealtimeSync, refreshSetupState } from '@/store/setupRealtimeStore'

const log = createLogger('profile-recovery-page')

export default function ProfileRecoveryPage({
  status,
  onRecovered,
  onRestart,
}: {
  status: ProfileRecoveryResponse
  onRecovered: () => void
  onRestart: () => void
}) {
  const { t } = useTranslation()
  const [form, setForm] = useState({ passphrase: '', submitting: false, errorKey: '' })
  const busy = form.submitting || status.state === 'recovering'
  const statusErrorKey =
    status.state === 'partially_recoverable'
      ? 'profileRecovery.partial'
      : status.state === 'failed'
        ? 'profileRecovery.failed'
        : ''
  const errorKey = status.restartRequired
    ? 'profileRecovery.restartRequired'
    : form.errorKey || statusErrorKey
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (busy || status.restartRequired || !status.canSubmitPassphrase || !form.passphrase) return
    setForm(value => ({ ...value, submitting: true, errorKey: '' }))
    try {
      await commands.unlockContent({ passphrase: form.passphrase })
      setForm({ passphrase: '', submitting: false, errorKey: '' })
      try {
        await ensureSetupRealtimeSync()
        await refreshSetupState()
      } catch {
        log.warn('Setup refresh pending after profile recovery')
      }
      onRecovered()
    } catch (error) {
      setForm(value => ({ ...value, submitting: false, errorKey: contentUnlockErrorKey(error) }))
    }
  }
  return (
    <AppStateShell
      title={t('profileRecovery.title')}
      description={t('profileRecovery.description')}
    >
      <form onSubmit={submit} className="mt-7 flex flex-col gap-4">
        {status.losses.length > 0 && !errorKey && (
          <p
            role="alert"
            className="rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-ui-body text-destructive"
          >
            {t('profileRecovery.partial')}
          </p>
        )}
        <Label htmlFor="recovery-passphrase">{t('unlock.passphraseModal.passphraseLabel')}</Label>
        <Input
          id="recovery-passphrase"
          type="password"
          autoComplete="off"
          autoFocus
          value={form.passphrase}
          disabled={busy || status.restartRequired || !status.canSubmitPassphrase}
          onChange={event =>
            setForm(value => ({ ...value, passphrase: event.target.value, errorKey: '' }))
          }
          aria-invalid={Boolean(form.errorKey)}
          aria-describedby="recovery-help"
        />
        {errorKey && (
          <p role="alert" className="text-ui-body text-destructive">
            {t(errorKey)}
          </p>
        )}
        {status.restartRequired ? (
          <Button type="button" onClick={onRestart} disabled={form.submitting}>
            {t('profileRecovery.restart')}
          </Button>
        ) : (
          <Button type="submit" disabled={busy || !status.canSubmitPassphrase || !form.passphrase}>
            {t(busy ? 'profileRecovery.recovering' : 'profileRecovery.submit')}
          </Button>
        )}
        <p id="recovery-help" className="text-ui-caption text-muted-foreground">
          {t('profileRecovery.help')}
        </p>
      </form>
    </AppStateShell>
  )
}
