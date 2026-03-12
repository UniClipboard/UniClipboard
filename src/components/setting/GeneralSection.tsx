import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
  Input,
} from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { SUPPORTED_LANGUAGES, type SupportedLanguage, getInitialLanguage } from '@/i18n'

export default function GeneralSection() {
  const { t } = useTranslation()
  const { setting, loading: settingLoading, updateGeneralSetting } = useSetting()
  const [autoStart, setAutoStart] = useState(setting?.general.auto_start ?? false)
  const [silentStart, setSilentStart] = useState(setting?.general.silent_start ?? false)
  const [language, setLanguage] = useState<SupportedLanguage>(() => {
    const backendLang = setting?.general.language
    const isValid = backendLang && SUPPORTED_LANGUAGES.includes(backendLang as SupportedLanguage)
    return isValid ? (backendLang as SupportedLanguage) : getInitialLanguage()
  })
  const [deviceName, setDeviceName] = useState(setting?.general.device_name ?? '')
  const [saving, setSaving] = useState(false)
  const isBusy = settingLoading || saving

  // 从配置中读取设置（auto_start 状态由后端管理，直接从 settings 读取）
  useEffect(() => {
    if (!setting?.general) return
    setAutoStart(setting.general.auto_start)
    setSilentStart(setting.general.silent_start)
    // Validate backend language value against supported languages
    const backendLang = setting.general.language
    const isValidLanguage =
      backendLang && SUPPORTED_LANGUAGES.includes(backendLang as SupportedLanguage)
    setLanguage(isValidLanguage ? (backendLang as SupportedLanguage) : getInitialLanguage())
    setDeviceName(setting.general.device_name ?? '')
  }, [setting])

  // 处理自启动开关变化（后端 update_settings 会自动调用 ApplyAutostartSetting）
  const handleAutoStartChange = async (checked: boolean) => {
    try {
      setSaving(true)
      await updateGeneralSetting({ auto_start: checked })
      setAutoStart(checked)
    } catch (error) {
      console.error('更改自启动状态失败:', error)
    } finally {
      setSaving(false)
    }
  }

  // 处理静默启动开关变化
  const handleSilentStartChange = async (checked: boolean) => {
    try {
      setSaving(true)
      // 更新设置和状态
      await updateGeneralSetting({ silent_start: checked })
      setSilentStart(checked)
    } catch (error) {
      console.error('更改静默启动状态失败:', error)
    } finally {
      setSaving(false)
    }
  }

  const handleLanguageChange = async (next: string) => {
    try {
      setSaving(true)
      const normalized = (next as SupportedLanguage) || getInitialLanguage()
      await updateGeneralSetting({ language: normalized })
      setLanguage(normalized)
    } catch (error) {
      console.error('更改语言失败:', error)
    } finally {
      setSaving(false)
    }
  }

  const handleDeviceNameChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const newName = e.target.value
    setDeviceName(newName)
  }

  const handleDeviceNameBlur = async () => {
    try {
      setSaving(true)
      await updateGeneralSetting({ device_name: deviceName })
    } catch (error) {
      console.error('更改设备名称失败:', error)
    } finally {
      setSaving(false)
    }
  }

  return (
    <>
      <SettingGroup title={t('settings.sections.general.startupTitle')}>
        <SettingRow
          label={t('settings.sections.general.deviceName.label')}
          description={t('settings.sections.general.deviceName.description')}
        >
          <div className="w-40">
            <Input
              value={deviceName}
              onChange={handleDeviceNameChange}
              onBlur={handleDeviceNameBlur}
              placeholder={t('settings.sections.general.deviceName.placeholder')}
              disabled={isBusy}
            />
          </div>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.general.autoStart.label')}
          description={t('settings.sections.general.autoStart.description')}
        >
          <Switch checked={autoStart} onCheckedChange={handleAutoStartChange} disabled={isBusy} />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.general.silentStart.label')}
          description={t('settings.sections.general.silentStart.description')}
        >
          <Switch
            checked={silentStart}
            onCheckedChange={handleSilentStartChange}
            disabled={isBusy}
          />
        </SettingRow>
      </SettingGroup>

      <SettingGroup title={t('settings.sections.general.language.title')}>
        <SettingRow
          label={t('settings.sections.general.language.label')}
          description={t('settings.sections.general.language.description')}
        >
          <div className="w-40">
            <Select value={language} onValueChange={handleLanguageChange} disabled={isBusy}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SUPPORTED_LANGUAGES.map(lang => (
                  <SelectItem key={lang} value={lang}>
                    {t(`language.${lang}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </SettingRow>
      </SettingGroup>
    </>
  )
}
