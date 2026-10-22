import { QRCodeSVG } from 'qrcode.react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ANDROID_RELEASES_URL, TESTFLIGHT_URL } from '@/lib/mobile-client-downloads'
import { Disclosure, Select, Text } from '@/ui-kit'

export function DevicesKitPhoneInstall({ installUrl }: { installUrl: string }) {
  const { t } = useTranslation()
  const [client, setClient] = useState('ios')
  const url =
    client === 'ios' ? TESTFLIGHT_URL : client === 'android' ? ANDROID_RELEASES_URL : installUrl
  return (
    <Disclosure label={t('devices.mobileSync.credential.noClient.title')}>
      <div className="flex flex-col gap-4">
        <Select
          label={t('devices.mobileSync.credential.noClient.title')}
          value={client}
          onValueChange={setClient}
          items={[
            { value: 'ios', label: t('devices.mobileSync.credential.noClient.tabs.ios') },
            { value: 'android', label: t('devices.mobileSync.credential.noClient.tabs.android') },
            ...(installUrl
              ? [
                  {
                    value: 'shortcut',
                    label: t('devices.mobileSync.credential.noClient.ios.shortcutFallback'),
                  },
                ]
              : []),
          ]}
        />
        <QRCodeSVG
          value={url}
          size={160}
          marginSize={4}
          className="max-w-full self-center shrink-0"
          aria-label={t('devices.mobileSync.credential.noClient.title')}
        />
        <Text role="caption" tone="secondary">
          {t(
            `devices.mobileSync.credential.noClient.${client === 'android' ? 'android.scanLabel' : client === 'shortcut' ? 'ios.shortcutFallback' : 'ios.scanLabel'}`
          )}
        </Text>
      </div>
    </Disclosure>
  )
}
