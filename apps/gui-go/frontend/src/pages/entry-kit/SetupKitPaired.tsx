import { useTranslation } from 'react-i18next'
import { Button, EntryPage, InsetList, Text } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

function shortDeviceId(deviceId?: string | null): string {
  if (!deviceId) return '---'
  if (deviceId.length <= 14) return deviceId
  return `${deviceId.slice(0, 6)}...${deviceId.slice(-4)}`
}

/** S9 · both devices are members of the space. */
export function SetupKitPaired({
  localDeviceName,
  peerDeviceId,
  onDone,
}: {
  localDeviceName?: string | null
  peerDeviceId?: string | null
  onDone: () => void
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.pairingComplete' })
  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: t('connected'), tone: 'positive', icon: 'shield' }}
        title={t('title')}
        actions={
          <span data-testid="setup-complete-done">
            <Button variant="primary" onClick={onDone}>
              {t('actions.done')}
            </Button>
          </span>
        }
      >
        <div data-testid="setup-pairing-complete">
          <InsetList
            label={t('title')}
            rows={[
              {
                id: 'local',
                title: localDeviceName || t('devices.thisDevice'),
                detail: t('devices.local'),
              },
              {
                id: 'peer',
                title: `${t('devices.peer')} · ${shortDeviceId(peerDeviceId)}`,
                detail: t('devices.joined'),
                tone: 'positive',
              },
            ]}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Text tone="secondary">{t('points.encrypted')}</Text>
          <Text tone="secondary">{t('points.sync')}</Text>
        </div>
      </EntryPage>
    </EntryKitFrame>
  )
}
