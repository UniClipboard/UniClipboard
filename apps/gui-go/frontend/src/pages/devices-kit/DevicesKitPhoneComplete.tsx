import { QRCodeSVG } from 'qrcode.react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import { Button, Disclosure, SettingsGroup, SettingsRow, TaskPanel, Text } from '@/ui-kit'
import { DevicesKitCopyButton } from './DevicesKitCopyButton'
import { DevicesKitPhoneInstall } from './DevicesKitPhoneInstall'

/** One-time connection material stays in this task's memory and is discarded on leaving. */
export function DevicesKitPhoneComplete({
  credential,
  onDone,
}: {
  credential: RegisterMobileDeviceResult
  onDone: () => void
}) {
  const { t } = useTranslation()
  const [showPassword, setShowPassword] = useState(false)
  return (
    <TaskPanel
      title={t('devices.mobileSync.deviceDialog.connectTitle', { label: credential.label })}
      description={t('devices.mobileSync.deviceDialog.scanInstruction')}
      actions={
        <Button variant="primary" onClick={onDone}>
          {t('history.composite.done')}
        </Button>
      }
    >
      {credential.connectUri && (
        <QRCodeSVG
          value={credential.connectUri}
          size={192}
          marginSize={4}
          className="max-w-full self-center shrink-0"
          aria-label={t('devices.mobileSync.credential.pair.qrAlt')}
        />
      )}
      <Text role="caption" tone="secondary">
        {t('devices.mobileSync.deviceDialog.wifiSameNetwork')}
      </Text>
      <Disclosure
        label={t('devices.mobileSync.deviceDialog.manualEntry.title')}
        defaultOpen={!credential.connectUri}
      >
        <div className="flex flex-col gap-4">
          <Text role="caption" tone="secondary">
            {t('devices.mobileSync.deviceDialog.manualEntry.oneTimeHint')}
          </Text>
          <SettingsGroup>
            <SettingsRow label={t('devices.mobileSync.deviceDialog.sections.serverAddress')}>
              <Text monospace truncate>
                {credential.baseUrl}
              </Text>
              <DevicesKitCopyButton value={credential.baseUrl} />
            </SettingsRow>
            <SettingsRow label={t('devices.mobileSync.add.username.label')}>
              <Text monospace>{credential.username}</Text>
              <DevicesKitCopyButton value={credential.username} />
            </SettingsRow>
            <SettingsRow label={t('devices.mobileSync.add.password.label')}>
              <Text monospace>{showPassword ? credential.password : '••••••••'}</Text>
              <Button variant="link" onClick={() => setShowPassword(value => !value)}>
                {t(showPassword ? 'unlock.passphraseModal.hide' : 'unlock.passphraseModal.show')}
              </Button>
              <DevicesKitCopyButton value={credential.password} />
            </SettingsRow>
          </SettingsGroup>
        </div>
      </Disclosure>
      <DevicesKitPhoneInstall installUrl={credential.installUrl} />
    </TaskPanel>
  )
}
