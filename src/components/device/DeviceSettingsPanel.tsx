import React, { useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import type { ContentTypes } from '@/api/p2p'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { fetchDeviceSyncSettings, updateDeviceSyncSettings } from '@/store/slices/devicesSlice'

interface DeviceSettingsPanelProps {
  deviceId: string
  deviceName: string
  globalAutoSyncOff?: boolean
  globalFileSyncOff?: boolean
}

/** Maps ContentTypes fields to i18n keys */
const contentTypeEntries: {
  field: keyof ContentTypes
  i18nKey: string
  status: 'editable' | 'coming_soon'
}[] = [
  { field: 'text', i18nKey: 'syncText', status: 'editable' },
  { field: 'image', i18nKey: 'syncImage', status: 'editable' },
  { field: 'file', i18nKey: 'syncFile', status: 'editable' },
  { field: 'link', i18nKey: 'syncLink', status: 'editable' },
  { field: 'code_snippet', i18nKey: 'syncCodeSnippet', status: 'coming_soon' },
  { field: 'rich_text', i18nKey: 'syncRichText', status: 'coming_soon' },
]

const DeviceSettingsPanel: React.FC<DeviceSettingsPanelProps> = ({
  deviceId,
  globalAutoSyncOff,
  globalFileSyncOff,
}) => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()

  const settings = useAppSelector(state => state.devices.deviceSyncSettings[deviceId])
  const isLoading = useAppSelector(
    state => state.devices.deviceSyncSettingsLoading[deviceId] ?? false
  )
  const isGlobalOff = globalAutoSyncOff ?? false
  const isGlobalFileSyncOff = globalFileSyncOff ?? false

  useEffect(() => {
    dispatch(fetchDeviceSyncSettings(deviceId))
  }, [dispatch, deviceId])

  const handleAutoSyncToggle = useCallback(() => {
    if (!settings) return
    dispatch(
      updateDeviceSyncSettings({
        peerId: deviceId,
        settings: { ...settings, auto_sync: !settings.auto_sync },
      })
    )
  }, [dispatch, deviceId, settings])

  const handleContentTypeToggle = useCallback(
    (field: keyof ContentTypes) => {
      if (!settings) return
      dispatch(
        updateDeviceSyncSettings({
          peerId: deviceId,
          settings: {
            ...settings,
            content_types: {
              ...settings.content_types,
              [field]: !settings.content_types[field],
            },
          },
        })
      )
    },
    [dispatch, deviceId, settings]
  )

  const handleRestoreDefaults = useCallback(async () => {
    await dispatch(updateDeviceSyncSettings({ peerId: deviceId, settings: null }))
    dispatch(fetchDeviceSyncSettings(deviceId))
  }, [dispatch, deviceId])

  // Loading skeleton
  if (isLoading && !settings) {
    return (
      <div className="space-y-6 animate-pulse">
        <div>
          <div className="flex items-center justify-between mb-2 px-1">
            <div className="h-4 w-24 bg-muted rounded" />
            <div className="h-5 w-28 bg-muted rounded" />
          </div>
          <div className="divide-y divide-border/40">
            {[1, 2, 3, 4].map(i => (
              <div key={i} className="flex items-center justify-between py-3 px-1">
                <div className="space-y-1.5">
                  <div className="h-4 w-32 bg-muted rounded" />
                  <div className="h-3 w-48 bg-muted rounded" />
                </div>
                <div className="w-9 h-5 bg-muted rounded-full" />
              </div>
            ))}
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div>
        <div className="flex items-center justify-between mb-2 px-1">
          <h4 className="text-sm font-medium text-muted-foreground uppercase tracking-wider">
            {t('devices.settings.sync.title')}
          </h4>
          <button
            type="button"
            onClick={handleRestoreDefaults}
            disabled={isGlobalOff || isLoading}
            className="text-xs px-2 py-1 rounded-md text-muted-foreground hover:bg-muted hover:text-foreground transition-colors disabled:opacity-50"
          >
            {t('devices.settings.sync.restoreDefaults')}
          </button>
        </div>

        <div className="divide-y divide-border/40">
          {/* Auto Sync toggle */}
          <div className="flex items-center justify-between py-3 px-1">
            <div className="pr-4">
              <div className="flex items-center gap-2">
                <h5 className="text-sm font-medium text-foreground">
                  {t('devices.settings.sync.rules.autoSync.title')}
                </h5>
              </div>
              <p className="text-xs mt-0.5 text-muted-foreground">
                {t('devices.settings.sync.rules.autoSync.description')}
              </p>
            </div>
            <label
              className={`flex items-center shrink-0 ${
                isGlobalOff || isLoading ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'
              }`}
            >
              <div className="relative">
                <input
                  type="checkbox"
                  className="sr-only peer"
                  checked={settings?.auto_sync ?? true}
                  onChange={handleAutoSyncToggle}
                  disabled={isGlobalOff || isLoading}
                />
                <div className="block w-9 h-5 rounded-full transition-colors bg-muted peer-checked:bg-primary" />
                <div className="absolute left-1 top-1 w-3 h-3 rounded-full transition-transform transform peer-checked:translate-x-4 bg-white" />
              </div>
            </label>
          </div>

          {/* Content type toggles */}
          {contentTypeEntries.map(({ field, i18nKey, status }) => {
            const isComingSoon = status === 'coming_soon'
            const isAutoSyncOff = !settings?.auto_sync
            const isGlobalFileSyncDisabled = field === 'file' && isGlobalFileSyncOff
            const isDisabled =
              isComingSoon || isAutoSyncOff || isGlobalOff || isGlobalFileSyncDisabled || isLoading

            return (
              <div key={field} className="flex items-center justify-between py-3 px-1">
                <div className="pr-4">
                  <div className="flex items-center gap-2">
                    <h5 className="text-sm font-medium text-foreground">
                      {t(`devices.settings.sync.rules.${i18nKey}.title`)}
                    </h5>
                    {isComingSoon && (
                      <span className="text-[10px] leading-none rounded px-1.5 py-1 bg-muted text-muted-foreground">
                        {t('devices.settings.badges.comingSoon')}
                      </span>
                    )}
                    {isGlobalFileSyncDisabled && !isComingSoon && (
                      <span className="text-[10px] leading-none rounded px-1.5 py-1 bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20">
                        {t('devices.settings.badges.globalFileSyncOff')}
                      </span>
                    )}
                  </div>
                  <p className="text-xs mt-0.5 text-muted-foreground">
                    {t(`devices.settings.sync.rules.${i18nKey}.description`)}
                  </p>
                </div>
                <label
                  className={`flex items-center shrink-0 ${
                    isDisabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'
                  }`}
                >
                  <div className="relative">
                    <input
                      type="checkbox"
                      className="sr-only peer"
                      checked={settings?.content_types[field] ?? true}
                      onChange={() => handleContentTypeToggle(field)}
                      disabled={isDisabled}
                    />
                    <div
                      className={`block w-9 h-5 rounded-full transition-colors bg-muted ${
                        isComingSoon
                          ? 'peer-checked:bg-muted-foreground/40'
                          : 'peer-checked:bg-primary'
                      }`}
                    />
                    <div
                      className={`absolute left-1 top-1 w-3 h-3 rounded-full transition-transform transform peer-checked:translate-x-4 ${
                        isComingSoon ? 'bg-muted-foreground/40' : 'bg-white'
                      }`}
                    />
                  </div>
                </label>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

export default DeviceSettingsPanel
