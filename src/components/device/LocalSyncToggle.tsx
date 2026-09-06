import { memo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Switch } from '@/components/ui/switch'
import { toast } from '@/components/ui/toast'
import { useSettingSelector } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'

const log = createLogger('local-device-sync')

function LocalSyncToggle({ kind }: { kind: 'sync' | 'file' }) {
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
  const title = t(`devices.panel.policies.${key}.title`)

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

  return (
    <div
      className={cn(
        'flex min-h-16 min-w-0 items-center gap-5 px-5 py-4 @md:px-6',
        !available && 'opacity-55'
      )}
    >
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-foreground">{title}</p>
        <p className="mt-1 text-xs leading-snug text-muted-foreground">
          {t(`devices.panel.policies.${key}.description`)}
        </p>
      </div>
      <Switch
        aria-label={title}
        aria-busy={saving}
        className={cn(
          'shrink-0',
          available && saving && 'disabled:cursor-pointer disabled:opacity-100'
        )}
        checked={checked}
        disabled={!available || saving}
        onCheckedChange={value => void save(value)}
      />
    </div>
  )
}

export default memo(LocalSyncToggle)
