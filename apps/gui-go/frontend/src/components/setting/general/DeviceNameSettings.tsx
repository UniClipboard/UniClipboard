import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingRow } from '@/components/setting/SettingRow'
import { Input } from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'

const log = createLogger('device-name-settings')

export function DeviceNameSettings() {
  const { t } = useTranslation()
  const { setting, updateGeneralSetting } = useSetting()
  const persistedName = setting?.general.deviceName ?? ''
  const [draft, setDraft] = useState<string | null>(null)
  const name = draft ?? persistedName

  const save = () => {
    if (name === persistedName) {
      setDraft(null)
      return
    }
    const submitted = name
    updateGeneralSetting({ deviceName: submitted }).then(
      () => setDraft(active => (active === submitted ? null : active)),
      (error: unknown) => {
        log.error({ err: error }, 'Failed to change device name')
        toast.error(t('settings.sections.general.saveError'))
      }
    )
  }

  return (
    <SettingRow
      label={t('settings.sections.general.deviceName.label')}
      description={t('settings.sections.general.deviceName.description')}
      className="pt-0"
    >
      <div className="w-40">
        <Input
          aria-label={t('settings.sections.general.deviceName.label')}
          value={name}
          onChange={event => setDraft(event.target.value)}
          onBlur={save}
          placeholder={t('settings.sections.general.deviceName.placeholder')}
        />
      </div>
    </SettingRow>
  )
}
