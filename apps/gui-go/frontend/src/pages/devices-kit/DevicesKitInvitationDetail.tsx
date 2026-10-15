import { useTranslation } from 'react-i18next'
import ChangePassphraseForm from '@/components/security/ChangePassphraseForm'
import {
  Button,
  EntityHeader,
  InlineNotice,
  ProgressBar,
  SettingsGroup,
  SettingsRow,
  Text,
} from '@/ui-kit'
import { invitationSummary } from './devices-kit-model'
import { DevicesKitCredentials } from './DevicesKitCredentials'
import type { DeviceInvitationRow } from './useDeviceInvitationRow'

/**
 * The invitation's detail (design DV4): the code to type on the other device, its expiry and the
 * passphrase hint. It is not a modal: the user can select another device while waiting, and the
 * row keeps counting down. Expired and failed invitations stay with Close and Regenerate.
 */
export function DevicesKitInvitationDetail({ row }: { row: DeviceInvitationRow }) {
  const { t } = useTranslation()
  const { state } = row
  const summary = invitationSummary(state, t)
  const live = state.step === 'invitation' && !!state.invitation && !state.expired
  const ended =
    state.step === 'failed' ||
    (state.step === 'invitation' && (state.expired || (!state.invitation && !!state.error)))

  const actions = live ? (
    <Button variant="destructive" busy={state.loading} onClick={() => void state.handleCancel()}>
      {t('devices.kit.cancelInvitation')}
    </Button>
  ) : ended ? (
    <>
      <Button disabled={state.loading} onClick={row.close}>
        {t('devices.addDevice.actions.close')}
      </Button>
      {state.issueRetryable !== false && (
        <Button
          variant="primary"
          icon="retry"
          busy={state.loading}
          onClick={() => void state.handleRegenerate()}
        >
          {t('devices.addDevice.actions.regenerate')}
        </Button>
      )}
    </>
  ) : state.step === 'credentials' || state.step === 'reset_passphrase' ? (
    // Re-pairing: the row can be dismissed at any step, as the old dialog could be closed.
    <Button disabled={state.loading} onClick={row.close}>
      {t('devices.addDevice.actions.close')}
    </Button>
  ) : undefined

  return (
    <>
      <EntityHeader
        icon="add"
        pending
        title={t(
          state.step === 'credentials'
            ? 'devices.addDevice.rePairing.title'
            : state.step === 'reset_passphrase'
              ? 'passphraseChange.title'
              : 'devices.addDevice.title'
        )}
        status={summary}
        description={
          state.step === 'credentials' ? t('devices.addDevice.rePairing.subtitle') : undefined
        }
        actions={actions}
      />
      {state.step === 'credentials' && <DevicesKitCredentials state={state} />}
      {state.step === 'reset_passphrase' && (
        <ChangePassphraseForm
          submitting={state.passphraseChangeSubmitting}
          onSubmittingChange={state.setPassphraseChangeSubmitting}
          onCancel={state.handleCancelPassphraseChange}
          onChanged={state.handlePassphraseChanged}
        />
      )}
      {state.step === 'failed' && (
        <InlineNotice
          tone="error"
          title={state.failureMessage}
          text={t('devices.addDevice.failed.networkHint')}
        />
      )}
      {state.step === 'invitation' && !state.invitation && state.error && (
        <InlineNotice tone="error" title={state.error} />
      )}
      {state.step === 'invitation' && (state.invitation || state.loading) && (
        <SettingsGroup>
          <SettingsRow description={t('devices.addDevice.subtitle')} />
          <SettingsRow>
            <span data-testid="add-device-invitation-code" aria-label={state.invitation?.code}>
              <Text monospace large tone={state.expired ? 'tertiary' : 'primary'}>
                {state.invitation ? state.display : '–––-–––'}
              </Text>
            </span>
          </SettingsRow>
          <SettingsRow label={summary.text}>
            {live && (
              <Button icon="copy" onClick={() => void state.handleCopy()}>
                {t(
                  state.copied
                    ? 'devices.addDevice.actions.copied'
                    : 'devices.addDevice.actions.copy'
                )}
              </Button>
            )}
          </SettingsRow>
        </SettingsGroup>
      )}
      {state.step === 'invitation' && (state.invitation || state.loading) && (
        <ProgressBar label={summary.text} value={state.invitation ? state.progress : undefined} />
      )}
      {(state.step === 'invitation' || state.step === 'failed') && (
        <InlineNotice tone="info" title={t('devices.addDevice.passphraseHint')} />
      )}
      {live && (
        <Text role="caption" tone="secondary">
          {t('devices.kit.waitHint')}
        </Text>
      )}
    </>
  )
}
