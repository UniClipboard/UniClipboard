import { useTranslation } from 'react-i18next'
import {
  useInitializeSpaceForm,
  type InitializeSpaceSubmit,
} from '@/pages/setup/useInitializeSpaceForm'
import { Button, EntryPage, FormField, InlineNotice } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

const FORM = 'setup-initialize-form'

/** S2 · create the space: device name and the space passphrase twice. */
export function SetupKitInitialize({
  onSubmit,
  onBack,
  loading,
}: {
  onSubmit: InitializeSpaceSubmit
  onBack: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.initializeSpace' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  const { t: tUnlock } = useTranslation(undefined, { keyPrefix: 'unlock.passphraseModal' })
  const form = useInitializeSpaceForm(onSubmit, loading)
  const reveal = { show: tUnlock('show'), hide: tUnlock('hide') }
  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: tWelcome('create.cta'), icon: 'shield' }}
        title={t('title')}
        description={t('subtitle')}
        back={{ label: t('actions.back'), onClick: onBack, disabled: loading }}
        actions={
          <Button
            data-testid="setup-initialize-submit"
            variant="primary"
            submitForm={FORM}
            busy={loading}
          >
            {t(loading ? 'actions.creating' : 'actions.submit')}
          </Button>
        }
      >
        <form id={FORM} onSubmit={event => void form.submit(event)}>
          {form.generalError && <InlineNotice tone="error" title={form.generalError} />}
          <div data-testid="device-name">
            <FormField
              id="device-name"
              label={t('labels.deviceName')}
              value={form.deviceName}
              onValueChange={form.setDeviceName}
              disabled={loading}
              placeholder={t('placeholders.deviceName')}
              error={form.deviceNameError ?? undefined}
            />
          </div>
          <div data-testid="pass1">
            <FormField
              id="pass1"
              label={t('labels.passphrase')}
              type="password"
              reveal={reveal}
              autoComplete="new-password"
              value={form.pass1}
              onValueChange={form.setPass1}
              disabled={loading}
              placeholder={t('placeholders.passphrase')}
              hint={t('hint')}
            />
          </div>
          <div data-testid="pass2">
            <FormField
              id="pass2"
              label={t('labels.passphraseConfirm')}
              type="password"
              reveal={reveal}
              autoComplete="new-password"
              value={form.pass2}
              onValueChange={form.setPass2}
              disabled={loading}
              placeholder={t('placeholders.passphraseConfirm')}
              error={form.passphraseError ?? undefined}
            />
          </div>
        </form>
      </EntryPage>
    </EntryKitFrame>
  )
}
