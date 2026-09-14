import { Loader2, LockKeyhole } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { getPassphraseChangeAvailabilityMessageKey } from '@/components/security/passphrase-change-availability'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

export default function RePairingCredentialsStep({
  invitationState,
  formId,
}: {
  invitationState: ReturnType<typeof useAddDeviceInvitation>
  formId?: string
}) {
  const { t } = useTranslation()
  const {
    error,
    loading,
    passphrase,
    passphraseChangeAvailability,
    setPassphrase,
    handleConfirmPassphrase,
    handleStartPassphraseChange,
  } = invitationState

  return (
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
          disabled={loading || !passphrase}
        >
          {loading && <Loader2 className="mr-2 size-4 animate-spin" />}
          {t(
            loading
              ? 'devices.addDevice.rePairing.submitting'
              : 'devices.addDevice.rePairing.submit'
          )}
        </Button>
      )}
      <Button
        type="button"
        variant="link"
        onClick={handleStartPassphraseChange}
        disabled={loading || passphraseChangeAvailability !== 'available'}
      >
        {t('passphraseChange.invitationAction')}
      </Button>
      {passphraseChangeAvailability !== 'available' && (
        <p className="text-center text-ui-caption-relaxed text-muted-foreground">
          {t(getPassphraseChangeAvailabilityMessageKey(passphraseChangeAvailability))}
        </p>
      )}
    </form>
  )
}
