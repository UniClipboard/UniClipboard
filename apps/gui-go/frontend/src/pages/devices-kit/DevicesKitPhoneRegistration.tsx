import { useTranslation } from 'react-i18next'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import { useMobileRegistration } from '@/hooks/useMobileRegistration'
import { Button, Disclosure, FormField, InlineNotice, TaskPanel, Text } from '@/ui-kit'

export function DevicesKitPhoneRegistration(props: {
  onBack: () => void
  onSuccess: (result: RegisterMobileDeviceResult) => void
  onBusyChange: (busy: boolean) => void
}) {
  const { t } = useTranslation()
  const form = useMobileRegistration(props)
  return (
    <TaskPanel
      title={t('devices.tasks.phoneNameTitle')}
      description={t('devices.tasks.phoneNameDescription')}
      stage={t('devices.tasks.step', { current: 2, total: 2 })}
      back={{ label: t('devices.tasks.back'), onClick: props.onBack, disabled: form.submitting }}
      actions={
        <Button
          variant="primary"
          busy={form.submitting}
          disabled={!form.label.trim()}
          submitForm="phone-registration"
        >
          {t('devices.mobileSync.add.submit')}
        </Button>
      }
    >
      <form
        id="phone-registration"
        className="flex flex-col gap-5"
        onSubmit={event => {
          event.preventDefault()
          if (!form.submitting) void form.handleSubmit()
        }}
      >
        <FormField
          id="mobile-sync-label"
          autoFocus
          label={t('devices.mobileSync.add.labelField.label')}
          value={form.label}
          onValueChange={value => form.dispatch({ type: 'edit', field: 'label', value })}
          placeholder={t('devices.mobileSync.add.labelField.placeholder')}
          disabled={form.submitting}
          error={form.fieldErrors.label}
        />
        <Disclosure
          label={t('devices.mobileSync.add.advanced.title')}
          defaultOpen={form.advancedOpen}
        >
          <div className="flex flex-col gap-4">
            <Text role="caption" tone="secondary">
              {t('devices.mobileSync.add.advanced.description')}
            </Text>
            <FormField
              id="mobile-sync-username"
              label={t('devices.mobileSync.add.username.label')}
              autoComplete="off"
              value={form.username}
              onValueChange={value => form.dispatch({ type: 'edit', field: 'username', value })}
              hint={t('devices.mobileSync.add.username.help')}
              disabled={form.submitting}
              error={form.fieldErrors.username}
            />
            <FormField
              id="mobile-sync-password"
              label={t('devices.mobileSync.add.password.label')}
              type="password"
              reveal={{
                show: t('unlock.passphraseModal.show'),
                hide: t('unlock.passphraseModal.hide'),
              }}
              autoComplete="new-password"
              value={form.password}
              onValueChange={value => form.dispatch({ type: 'edit', field: 'password', value })}
              hint={t('devices.mobileSync.add.password.help')}
              disabled={form.submitting}
              error={form.fieldErrors.password}
            />
          </div>
        </Disclosure>
        {form.formError && <InlineNotice tone="error" title={form.formError} />}
      </form>
    </TaskPanel>
  )
}
