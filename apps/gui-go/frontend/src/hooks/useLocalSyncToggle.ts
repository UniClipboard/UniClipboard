import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from '@/components/ui/toast'
import { useSettingSelector } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'

const log = createLogger('local-device-sync')

/**
 * One of this device's two sync switches (all sync, file sync). File sync is only available while
 * sync is on and reads off with it. The switch owns its saving state; the setting provider owns
 * the value.
 */
export function useLocalSyncToggle(kind: 'sync' | 'file') {
  const { t } = useTranslation()
  const available = useSettingSelector(
    ({ setting }) => setting !== null && (kind === 'sync' || setting.sync.syncEnabled !== false)
  )
  const checked = useSettingSelector(
    ({ setting }) =>
      setting !== null &&
      setting.sync.syncEnabled !== false &&
      (kind === 'sync' || setting.fileSync?.fileSyncEnabled !== false)
  )
  const updateSync = useSettingSelector(context => context.updateSyncSetting)
  const updateFile = useSettingSelector(context => context.updateFileSyncSetting)
  const [saving, setSaving] = useState(false)
  const savingRef = useRef(false)
  const key = kind === 'sync' ? 'syncEnabled' : 'fileSync'

  const save = async (value: boolean) => {
    if (!available || savingRef.current) return
    savingRef.current = true
    setSaving(true)
    try {
      if (kind === 'sync') await updateSync({ syncEnabled: value })
      else await updateFile({ fileSyncEnabled: value })
    } catch (err) {
      log.error({ err }, 'failed to update device sync setting')
      toast.error(t('devices.settings.sync.updateFailed'))
    } finally {
      savingRef.current = false
      setSaving(false)
    }
  }

  return {
    available,
    checked,
    saving,
    title: t(`devices.panel.policies.${key}.title`),
    description: t(`devices.panel.policies.${key}.description`),
    save,
  }
}
