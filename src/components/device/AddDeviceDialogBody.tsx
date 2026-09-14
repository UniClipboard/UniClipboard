import { AlertCircle, CheckCircle2, Loader2, LockKeyhole, RefreshCw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { AddDeviceInvitation } from '@/components/device/AddDeviceInvitation'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

export function AddDeviceDialogBody({
  invitationState,
  formId,
}: {
  invitationState: ReturnType<typeof useAddDeviceInvitation>
  formId?: string
}) {
  const { t } = useTranslation()
  const {
    invitation,
    loading,
    error,
    step,
    passphrase,
    remaining,
    expired,
    progress,
    display,
    failureMessage,
    setPassphrase,
    handleRegenerate,
    handleConfirmPassphrase,
  } = invitationState
  let body: React.ReactNode = null
  if (step === 'credentials') {
    body = (
      <form
        id={formId}
        data-testid="re-pairing-passphrase-step"
        className="flex flex-col gap-4 py-3"
        onSubmit={handleConfirmPassphrase}
      >
        <div className="flex items-start gap-3 rounded-lg border border-primary/20 bg-primary/5 p-3 text-ui-body text-foreground">
          <LockKeyhole className="mt-0.5 size-4 shrink-0 text-primary" />
          <span>{t('devices.addDevice.rePairing.description')}</span>
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="re-pairing-passphrase">
            {t('devices.addDevice.rePairing.passphraseLabel')}
          </Label>
          <Input
            id="re-pairing-passphrase"
            type="password"
            autoComplete="current-password"
            autoFocus
            value={passphrase}
            onChange={event => setPassphrase(event.target.value)}
            placeholder={t('devices.addDevice.rePairing.passphrasePlaceholder')}
            disabled={loading}
          />
        </div>
        {error && <p className="text-ui-body font-medium text-destructive">{error}</p>}
        {!formId && (
          <Button
            type="submit"
            data-testid="re-pairing-confirm-passphrase"
            disabled={loading || !passphrase.trim()}
          >
            {loading && <Loader2 className="mr-2 size-4 animate-spin" />}
            {loading
              ? t('devices.addDevice.rePairing.submitting')
              : t('devices.addDevice.rePairing.submit')}
          </Button>
        )}
      </form>
    )
  } else if (step === 'success') {
    body = (
      <div data-testid="add-device-success" className="flex flex-col items-center gap-3 py-8">
        <div className="flex size-14 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">
          <CheckCircle2 className="size-8" />
        </div>
        <div className="text-center">
          <p className="text-ui-section font-semibold text-foreground">
            {t('devices.addDevice.success.title')}
          </p>
          <p className="mt-1 text-ui-body text-muted-foreground">
            {t('devices.addDevice.success.subtitle')}
          </p>
        </div>
      </div>
    )
  } else if (step === 'failed') {
    body = (
      <div className="flex flex-col items-center gap-3 py-6">
        <div className="flex size-12 items-center justify-center rounded-full bg-destructive/15 text-destructive">
          <AlertCircle className="size-7" />
        </div>
        <div className="text-center">
          <p className="text-ui-section font-semibold text-foreground">
            {t('devices.addDevice.failed.title')}
          </p>
          <p className="mt-1 text-ui-body text-muted-foreground">{failureMessage}</p>
          <p className="mt-3 text-ui-caption text-muted-foreground/70">
            {t('devices.addDevice.failed.networkHint')}
          </p>
        </div>
      </div>
    )
  } else if (loading && !invitation) {
    body = (
      <div className="flex items-center justify-center gap-3 py-12 text-ui-body text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        {t('devices.addDevice.loading')}
      </div>
    )
  } else if (error && !invitation) {
    body = (
      <div className="flex flex-col items-center gap-3 py-10">
        <p className="text-ui-body text-destructive">{error}</p>
        {!formId && (
          <Button variant="outline" size="sm" onClick={handleRegenerate} disabled={loading}>
            <RefreshCw className="mr-2 size-3.5" />
            {t('devices.addDevice.actions.regenerate')}
          </Button>
        )}
      </div>
    )
  } else if (invitation) {
    body = (
      <AddDeviceInvitation
        invitation={invitation}
        expired={expired}
        display={display}
        progress={progress}
        remaining={remaining}
      />
    )
  }

  return body
}
