import { useId } from 'react'
import { useTranslation } from 'react-i18next'
import { usePeerAddress } from '@/hooks/usePeerAddress'
import { Button, FormField, InlineNotice, SettingsGroup } from '@/ui-kit'

export function DevicesKitPeerAddress({ deviceId }: { deviceId: string }) {
  const { t } = useTranslation()
  const address = usePeerAddress(deviceId)
  const formId = useId()
  return (
    <SettingsGroup title={t('devices.directAddress.title')}>
      <form
        id={formId}
        className="space-y-3"
        onSubmit={event => {
          event.preventDefault()
          void address.save()
        }}
      >
        <FormField
          label={t('devices.directAddress.ip')}
          value={address.ip}
          onValueChange={address.setIp}
          autoComplete="off"
          disabled={address.disabled}
          hint={t('devices.directAddress.hint')}
          error={address.ipError}
        />
        <FormField
          label={t('devices.directAddress.port')}
          value={address.port}
          onValueChange={address.setPort}
          autoComplete="off"
          disabled={address.disabled}
          error={address.portError}
        />
        <div className="flex gap-2">
          <Button
            submitForm={formId}
            variant="primary"
            disabled={address.disabled || !address.canSave}
            busy={address.busy}
          >
            {t(address.busy ? 'devices.directAddress.saving' : 'devices.directAddress.save')}
          </Button>
          <Button
            disabled={address.disabled || !address.canClear}
            onClick={() => {
              void address.clear()
            }}
          >
            {t('devices.directAddress.clear')}
          </Button>
        </div>
      </form>
      {address.error && <InlineNotice tone="error" title={address.error} />}
    </SettingsGroup>
  )
}
