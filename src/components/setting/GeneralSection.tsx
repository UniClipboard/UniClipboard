import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import * as storageApi from '@/api/storage'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
  Input,
  Button,
} from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { SUPPORTED_LANGUAGES, type SupportedLanguage, getInitialLanguage } from '@/i18n'
import { createLogger } from '@/lib/logger'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'

const log = createLogger('general-section')

export default function GeneralSection() {
  const { t } = useTranslation()
  const { setting, loading: settingLoading, updateGeneralSetting, updateAutostart } = useSetting()
  const [autoStart, setAutoStart] = useState(setting?.general.autoStart ?? false)
  const [silentStart, setSilentStart] = useState(setting?.general.silentStart ?? false)
  const [telemetryEnabled, setTelemetryEnabled] = useState(
    setting?.general.telemetryEnabled ?? true
  )
  const [usageAnalyticsEnabled, setUsageAnalyticsEnabled] = useState(
    setting?.general.usageAnalyticsEnabled ?? true
  )
  const [language, setLanguage] = useState<SupportedLanguage>(() => {
    const backendLang = setting?.general.language
    const isValid = backendLang && SUPPORTED_LANGUAGES.includes(backendLang as SupportedLanguage)
    return isValid ? (backendLang as SupportedLanguage) : getInitialLanguage()
  })
  const [deviceName, setDeviceName] = useState(setting?.general.deviceName ?? '')
  const [saving, setSaving] = useState(false)
  const isBusy = settingLoading || saving

  // 从配置中读取设置（auto_start 展示值来自持久化设置；切换走专用命令，见下）
  useEffect(() => {
    if (!setting?.general) return
    setAutoStart(setting.general.autoStart)
    setSilentStart(setting.general.silentStart)
    setTelemetryEnabled(setting.general.telemetryEnabled)
    setUsageAnalyticsEnabled(setting.general.usageAnalyticsEnabled ?? true)
    // Validate backend language value against supported languages
    const backendLang = setting.general.language
    const isValidLanguage =
      backendLang && SUPPORTED_LANGUAGES.includes(backendLang as SupportedLanguage)
    setLanguage(isValidLanguage ? (backendLang as SupportedLanguage) : getInitialLanguage())
    setDeviceName(setting.general.deviceName ?? '')
  }, [setting])

  // 处理自启动开关变化。走专用命令 update_autostart：在同一后端调用里持久化
  // auto_start 偏好并应用 OS 启动项注册，失败会回滚设置。不能走通用
  // updateGeneralSetting —— 设置链路不会触发任何 OS 副作用。
  const handleAutoStartChange = async (checked: boolean) => {
    try {
      setSaving(true)
      await updateAutostart(checked)
      setAutoStart(checked)
    } catch (error) {
      log.error({ err: error }, '更改自启动状态失败')
    } finally {
      setSaving(false)
    }
  }

  // 处理静默启动开关变化
  const handleSilentStartChange = async (checked: boolean) => {
    try {
      setSaving(true)
      // 更新设置和状态
      await updateGeneralSetting({ silentStart: checked })
      setSilentStart(checked)
    } catch (error) {
      log.error({ err: error }, '更改静默启动状态失败')
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
      log.error({ err: error }, '更改语言失败')
    } finally {
      setSaving(false)
    }
  }

  const handleDeviceNameChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const newName = e.target.value
    setDeviceName(newName)
  }

  const handleTelemetryChange = async (checked: boolean) => {
    try {
      setSaving(true)
      await updateGeneralSetting({ telemetryEnabled: checked })
      setTelemetryEnabled(checked)
    } catch (error) {
      log.error({ err: error }, '更改诊断数据设置失败')
    } finally {
      setSaving(false)
    }
  }

  const handleUsageAnalyticsChange = async (checked: boolean) => {
    try {
      setSaving(true)
      await updateGeneralSetting({ usageAnalyticsEnabled: checked })
      setUsageAnalyticsEnabled(checked)
    } catch (error) {
      log.error({ err: error }, '更改使用情况分析设置失败')
    } finally {
      setSaving(false)
    }
  }

  const handleDeviceNameBlur = async () => {
    try {
      setSaving(true)
      await updateGeneralSetting({ deviceName: deviceName })
    } catch (error) {
      log.error({ err: error }, '更改设备名称失败')
    } finally {
      setSaving(false)
    }
  }

  const handleOpenLogsDir = async () => {
    try {
      await storageApi.openLogsDirectory()
    } catch (error) {
      log.error({ err: error }, '打开日志目录失败')
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

      <SettingGroup title={t('settings.sections.general.telemetry.title')}>
        <SettingRow
          label={t('settings.sections.general.telemetry.diagnostics.label')}
          description={t('settings.sections.general.telemetry.diagnostics.description')}
        >
          <Switch
            checked={telemetryEnabled}
            onCheckedChange={handleTelemetryChange}
            disabled={isBusy}
          />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.general.telemetry.usageAnalytics.label')}
          description={t('settings.sections.general.telemetry.usageAnalytics.description')}
        >
          <Switch
            checked={usageAnalyticsEnabled}
            onCheckedChange={handleUsageAnalyticsChange}
            disabled={isBusy}
          />
        </SettingRow>
      </SettingGroup>

      <SettingGroup title={t('settings.sections.general.logsDirectory.title')}>
        <SettingRow
          label={t('settings.sections.general.logsDirectory.label')}
          description={t('settings.sections.general.logsDirectory.description')}
        >
          <Button variant="outline" size="sm" onClick={handleOpenLogsDir}>
            {t('settings.sections.general.logsDirectory.button')}
          </Button>
        </SettingRow>
      </SettingGroup>
    </>
  )
}
