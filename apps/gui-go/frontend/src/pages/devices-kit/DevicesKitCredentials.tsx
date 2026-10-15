import { useId } from 'react'
import { useTranslation } from 'react-i18next'
import { getPassphraseChangeAvailabilityMessageKey } from '@/components/security/passphrase-change-availability'
import type { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'
import { Button, ButtonRow, FormField, InlineNotice, Text } from '@/ui-kit'

/**
 * The one-time passphrase confirmation an upgraded profile needs before it can invite (the only
 * blocking step of an invitation), shown in the invitation's detail.
 */
export function DevicesKitCredentials({
  state,
}: {
  state: ReturnType<typeof useAddDeviceInvitation>
}) {
  const { t } = useTranslation()
  const { t: tUnlock } = useTranslation(undefined, { keyPrefix: 'unlock.passphraseModal' })
  const formId = useId()
  const available = state.passphraseChangeAvailability === 'available'
  return (
    <form
      id={formId}
      data-testid="re-pairing-passphrase-step"
      onSubmit={event => void state.handleConfirmPassphrase(event)}
      style={{ display: 'contents' }}
    >
      <InlineNotice tone="info" title={t('devices.addDevice.rePairing.description')} />
      <FormField
        id="re-pairing-passphrase"
        label={t('devices.addDevice.rePairing.passphraseLabel')}
        type="password"
        reveal={{ show: tUnlock('show'), hide: tUnlock('hide') }}
        autoComplete="current-password"
        autoFocus
        value={state.passphrase}
        onValueChange={state.setPassphrase}
        placeholder={t('devices.addDevice.rePairing.passphrasePlaceholder')}
        disabled={state.loading}
        error={state.error ?? undefined}
      />
      <ButtonRow>
        <Button
          data-testid="re-pairing-confirm-passphrase"
          variant="primary"
          submitForm={formId}
          busy={state.loading}
          disabled={!state.passphrase}
        >
          {t(
            state.loading
              ? 'devices.addDevice.rePairing.submitting'
              : 'devices.addDevice.rePairing.submit'
          )}
        </Button>
        <Button
          variant="link"
          disabled={state.loading || !available}
          onClick={state.handleStartPassphraseChange}
        >
          {t('passphraseChange.invitationAction')}
        </Button>
      </ButtonRow>
      {!available && (
        <Text role="caption" tone="secondary">
          {t(getPassphraseChangeAvailabilityMessageKey(state.passphraseChangeAvailability))}
        </Text>
      )}
    </form>
  )
}
