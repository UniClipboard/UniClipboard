import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { FullInvitationField } from '@/components/FullInvitationField'
import type { useSwitchSpace } from '@/hooks/useSwitchSpace'
import {
  shortInvitationInput,
  INVITATION_CODE_LENGTH,
  formatInvitationCode,
} from '@/lib/invitation-code'
import {
  Button,
  CodeField,
  FormField,
  InlineNotice,
  SettingsGroup,
  SettingsRow,
  TaskPanel,
  Text,
} from '@/ui-kit'

export function DevicesKitJoinInput({
  model,
  onBack,
}: {
  model: ReturnType<typeof useSwitchSpace>
  onBack: () => void
}) {
  const { t } = useTranslation()
  const [confirm, setConfirm] = useState(false)
  const title = t(confirm ? 'devices.tasks.joinConfirmTitle' : 'devices.tasks.joinCodeTitle')
  const description = t(
    confirm ? 'devices.tasks.joinConfirmDescription' : 'devices.tasks.joinCodeDescription'
  )
  return (
    <TaskPanel
      title={title}
      description={description}
      stage={t('devices.tasks.step', { current: confirm ? 2 : 1, total: 2 })}
      back={{
        label: t(confirm ? 'devices.tasks.back' : 'devices.switchSpace.backToDevices'),
        onClick: confirm ? () => setConfirm(false) : onBack,
      }}
      actions={
        <Button
          variant="primary"
          submitForm="switch-space-form"
          disabled={confirm ? !model.canSubmit : !model.codeComplete}
        >
          {t(confirm ? 'devices.switchSpace.actions.switch' : 'devices.tasks.continue')}
        </Button>
      }
    >
      <form
        id="switch-space-form"
        className="flex flex-col gap-5"
        onSubmit={event => {
          event.preventDefault()
          if (confirm) void model.handleSubmit()
          else if (model.codeComplete) setConfirm(true)
        }}
      >
        {confirm ? (
          <>
            <SettingsGroup>
              <SettingsRow label={t('devices.switchSpace.labels.code')}>
                <Text monospace>{formatInvitationCode(model.code)}</Text>
              </SettingsRow>
            </SettingsGroup>
            <FormField
              id="switch-pass"
              label={t('devices.switchSpace.labels.newPassphrase')}
              type="password"
              reveal={{
                show: t('unlock.passphraseModal.show'),
                hide: t('unlock.passphraseModal.hide'),
              }}
              autoComplete="off"
              autoFocus
              value={model.pass}
              onValueChange={model.setPass}
              placeholder={t('devices.switchSpace.placeholders.newPassphrase')}
            />
            <InlineNotice tone="info" title={t('devices.switchSpace.subtitle')} />
          </>
        ) : (
          <>
            <CodeField
              id="switch-code"
              label={t('devices.switchSpace.labels.code')}
              length={INVITATION_CODE_LENGTH}
              value={shortInvitationInput(model.code)}
              onValueChange={value => {
                model.setCode(value)
                model.setPass('')
              }}
              autoFocus
            />
            <FullInvitationField
              id="switch-full-invitation"
              value={model.code}
              onChange={value => {
                model.setCode(value)
                model.setPass('')
              }}
            />
          </>
        )}
      </form>
    </TaskPanel>
  )
}
