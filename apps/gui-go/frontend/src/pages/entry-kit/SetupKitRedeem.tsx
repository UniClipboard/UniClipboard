import { useTranslation } from 'react-i18next'
import { INVITATION_CODE_LENGTH } from '@/lib/invitation-code'
import {
  useRedeemInvitationForm,
  type RedeemInvitationSubmit,
} from '@/pages/setup/useRedeemInvitationForm'
import { Button, CodeField, EntryPage, FormField, InlineNotice, Text } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

const FORM = 'setup-redeem-form'

/** S6 · join an existing space: the six-digit invitation code, then the space passphrase. */
export function SetupKitRedeem({
  onSubmit,
  onBack,
  loading,
}: {
  onSubmit: RedeemInvitationSubmit
  onBack: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.redeemInvitation' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  const { t: tUnlock } = useTranslation(undefined, { keyPrefix: 'unlock.passphraseModal' })
  const form = useRedeemInvitationForm(onSubmit, loading)
  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: tWelcome('join.cta'), icon: 'phone' }}
        title={t('title')}
        description={t('subtitle')}
        back={{ label: t('actions.back'), onClick: onBack, disabled: loading }}
        actions={
          <Button
            data-testid="setup-redeem-submit"
            variant="primary"
            submitForm={FORM}
            busy={loading}
            disabled={!form.canSubmit}
          >
            {t(loading ? 'actions.joining' : 'actions.submit')}
          </Button>
        }
      >
        <form id={FORM} onSubmit={event => void form.submit(event)}>
          {form.errorMessage && <InlineNotice tone="error" title={form.errorMessage} />}
          <div data-testid="setup-redeem-code">
            <CodeField
              id="join-code"
              label={t('labels.code')}
              length={INVITATION_CODE_LENGTH}
              value={form.code}
              onValueChange={form.setCode}
              invalid={form.codeInvalid}
              disabled={loading}
              autoFocus
            />
          </div>
          {form.codeComplete && (
            <div data-testid="join-pass">
              <FormField
                id="join-pass"
                label={t('labels.passphrase')}
                type="password"
                reveal={{ show: tUnlock('show'), hide: tUnlock('hide') }}
                autoComplete="off"
                autoFocus
                value={form.pass}
                onValueChange={form.setPass}
                disabled={loading}
                placeholder={t('placeholders.passphrase')}
              />
            </div>
          )}
          <Text role="caption" tone="secondary">
            {t('hint')}
          </Text>
        </form>
      </EntryPage>
    </EntryKitFrame>
  )
}
