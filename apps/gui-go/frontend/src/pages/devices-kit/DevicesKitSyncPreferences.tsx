import { useTranslation } from 'react-i18next'
import { contentTypeEntries } from '@/components/device/device-utils'
import {
  DIRECTION_VALUES,
  getSyncDirection,
  SYNC_DIRECTIONS,
  type SyncDirection,
} from '@/components/device/sync-direction'
import type { usePeerSyncPreferences } from '@/hooks/usePeerSyncPreferences'
import { Button, InlineNotice, Select, SettingsGroup, SettingsRow, Switch, Text } from '@/ui-kit'

const MODE_KEYS: Record<SyncDirection, string> = {
  both: 'devices.settings.sync.modes.both',
  send_only: 'devices.settings.sync.modes.sendOnly',
  receive_only: 'devices.settings.sync.modes.receiveOnly',
  off: 'devices.settings.sync.modes.off',
}

/**
 * Sync with one peer (design DV1): the master switch, then one pop-up per content type with the
 * four directions, a summary and Restore Defaults. Global sync off locks everything; global file
 * sync off locks the Files pop-up.
 */
export function DevicesKitSyncPreferences({
  deviceName,
  sync,
  globalSyncOff,
  globalFileSyncOff,
}: {
  deviceName: string
  sync: ReturnType<typeof usePeerSyncPreferences>
  globalSyncOff: boolean
  globalFileSyncOff: boolean
}) {
  const { t } = useTranslation()
  const { preferences, sendEnabled, receiveEnabled } = sync
  const loading = !preferences
  const syncEnabled = sendEnabled || receiveEnabled
  const directions = contentTypeEntries.map(({ field }) =>
    getSyncDirection(
      sendEnabled && (preferences?.sendContentTypes?.[field] ?? true),
      receiveEnabled && (preferences?.receiveContentTypes?.[field] ?? true)
    )
  )
  const allBoth = directions.every(direction => direction === 'both')
  const modeItems = SYNC_DIRECTIONS.map(mode => ({
    value: mode,
    label: t(MODE_KEYS[mode], { deviceName }),
  }))
  const syncLabel = t('devices.settings.sync.syncWithDevice', { deviceName })

  return (
    <>
      {globalSyncOff && <InlineNotice tone="info" title={t('devices.thisDevice.syncPaused')} />}
      <SettingsGroup>
        <SettingsRow
          label={syncLabel}
          description={
            loading
              ? undefined
              : t(
                  syncEnabled
                    ? 'devices.settings.sync.enabledDescription'
                    : 'devices.settings.sync.disabledDescription'
                )
          }
        >
          <Switch
            label={syncLabel}
            labelHidden
            checked={syncEnabled}
            disabled={globalSyncOff || loading}
            onCheckedChange={sync.setSyncEnabled}
          />
        </SettingsRow>
      </SettingsGroup>
      <SettingsGroup title={t('devices.settings.sync.customize')}>
        <SettingsRow
          label={t(
            syncEnabled && allBoth
              ? 'devices.settings.sync.allBoth'
              : 'devices.settings.sync.customized'
          )}
          description={t('devices.settings.sync.customizeDescription')}
        >
          <Button
            variant="subtle"
            size="small"
            icon="retry"
            disabled={globalSyncOff || loading}
            onClick={() => void sync.restoreDefaults()}
          >
            {t('devices.settings.sync.restoreDefaults')}
          </Button>
        </SettingsRow>
        {contentTypeEntries.map(({ field, status }, index) => {
          const label = t(`devices.settings.sync.typeLabels.${field}`)
          const fileSyncOff = field === 'file' && globalFileSyncOff
          const unavailable = status === 'coming_soon'
          return (
            <SettingsRow
              key={field}
              label={label}
              labelExtra={
                fileSyncOff || unavailable ? (
                  <>
                    {' '}
                    <Text role="caption" tone="secondary">
                      {t(
                        fileSyncOff
                          ? 'devices.settings.badges.globalFileSyncOff'
                          : 'devices.settings.badges.comingSoon'
                      )}
                    </Text>
                  </>
                ) : undefined
              }
            >
              <Select
                label={t('devices.settings.sync.directionLabel', { contentType: label })}
                items={modeItems}
                value={directions[index]}
                disabled={!syncEnabled || globalSyncOff || fileSyncOff || unavailable || loading}
                onValueChange={value => {
                  const next = DIRECTION_VALUES[value as SyncDirection]
                  sync.setContentDirection(field, next.send, next.receive)
                }}
              />
            </SettingsRow>
          )
        })}
      </SettingsGroup>
    </>
  )
}
