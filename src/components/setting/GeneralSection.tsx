import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { exportLogs, updateDebugMode } from '@/api/daemon/diagnostics'
import * as storageApi from '@/api/storage'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
  Input,
  Button,
} from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { SUPPORTED_LANGUAGES, type SupportedLanguage, getInitialLanguage } from '@/i18n'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'

const log = createLogger('general-section')

export default function GeneralSection() {
  const { t } = useTranslation()
  const {
    setting,
    loading: settingLoading,
    reloadSetting,
    updateGeneralSetting,
    updateAutostart,
  } = useSetting()
  const [autoStart, setAutoStart] = useState(setting?.general.autoStart ?? false)
  const [silentStart, setSilentStart] = useState(setting?.general.silentStart ?? false)
  const [telemetryEnabled, setTelemetryEnabled] = useState(
    setting?.general.telemetryEnabled ?? true
  )
  const [usageAnalyticsEnabled, setUsageAnalyticsEnabled] = useState(
    setting?.general.usageAnalyticsEnabled ?? true
  )
  const [debugMode, setDebugMode] = useState(setting?.general.debugMode ?? false)
  const [debugConfirmOpen, setDebugConfirmOpen] = useState(false)
  const [exportPath, setExportPath] = useState<string | null>(null)
  const [exportingLogs, setExportingLogs] = useState(false)
  const [language, setLanguage] = useState<SupportedLanguage>(() => {
    const backendLang = setting?.general.language
    const isValid = backendLang && SUPPORTED_LANGUAGES.includes(backendLang as SupportedLanguage)
    return isValid ? (backendLang as SupportedLanguage) : getInitialLanguage()
  })
  const [deviceName, setDeviceName] = useState(setting?.general.deviceName ?? '')
  const [saving, setSaving] = useState(false)
  const isBusy = settingLoading || saving

  // Read display state from persisted settings. Autostart still uses the
  // dedicated host command because it applies OS launch registration.
  useEffect(() => {
    if (!setting?.general) return
    setAutoStart(setting.general.autoStart)
    setSilentStart(setting.general.silentStart)
    setTelemetryEnabled(setting.general.telemetryEnabled)
    setUsageAnalyticsEnabled(setting.general.usageAnalyticsEnabled ?? true)
    setDebugMode(setting.general.debugMode ?? false)
    // Validate backend language value against supported languages
    const backendLang = setting.general.language
    const isValidLanguage =
      backendLang && SUPPORTED_LANGUAGES.includes(backendLang as SupportedLanguage)
    setLanguage(isValidLanguage ? (backendLang as SupportedLanguage) : getInitialLanguage())
    setDeviceName(setting.general.deviceName ?? '')
  }, [setting])

  const handleAutoStartChange = async (checked: boolean) => {
    try {
      setSaving(true)
      await updateAutostart(checked)
      setAutoStart(checked)
    } catch (error) {
      log.error({ err: error }, 'Failed to change autostart setting')
      toast.error(t('settings.sections.general.saveError'))
    } finally {
      setSaving(false)
    }
  }

  const handleSilentStartChange = async (checked: boolean) => {
    try {
      setSaving(true)
      // 更新设置和状态
      await updateGeneralSetting({ silentStart: checked })
      setSilentStart(checked)
    } catch (error) {
      log.error({ err: error }, 'Failed to change silent-start setting')
      toast.error(t('settings.sections.general.saveError'))
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
      log.error({ err: error }, 'Failed to change language setting')
      toast.error(t('settings.sections.general.saveError'))
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
      log.error({ err: error }, 'Failed to change diagnostics setting')
      toast.error(t('settings.sections.general.saveError'))
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
      log.error({ err: error }, 'Failed to change usage analytics setting')
      toast.error(t('settings.sections.general.saveError'))
    } finally {
      setSaving(false)
    }
  }

  const persistDebugMode = async (enabled: boolean) => {
    try {
      setSaving(true)
      const result = await updateDebugMode(enabled)
      await reloadSetting()
      setDebugMode(result.debugMode)
      if (result.restartRequired) {
        toast.message(t('settings.sections.general.logs.debug.restartToast'))
      }
    } catch (error) {
      log.error({ err: error }, 'Failed to change debug mode')
      toast.error(t('settings.sections.general.logs.debug.error'))
    } finally {
      setSaving(false)
    }
  }

  const handleDebugModeChange = (checked: boolean) => {
    if (checked) {
      setDebugConfirmOpen(true)
    } else {
      void persistDebugMode(false)
    }
  }

  const handleConfirmDebugMode = async () => {
    setDebugConfirmOpen(false)
    try {
      setSaving(true)
      const result = await updateDebugMode(true)
      await reloadSetting()
      setDebugMode(result.debugMode)
      // Debug mode changes the log profile, which both the daemon and the GUI
      // read only at process start. Restart the daemon first so the engine —
      // the primary log producer — picks up the debug profile, then restart the
      // GUI. restartApp() exits this process, so code after it is unreachable on
      // the happy path.
      await commands.restartDaemon()
      await commands.restartApp()
    } catch (error) {
      log.error({ err: error }, 'Failed to enable debug mode and restart')
      toast.error(t('settings.sections.general.logs.debug.error'))
      setSaving(false)
    }
  }

  const handleDeviceNameBlur = async () => {
    try {
      setSaving(true)
      await updateGeneralSetting({ deviceName: deviceName })
    } catch (error) {
      log.error({ err: error }, 'Failed to change device name')
      toast.error(t('settings.sections.general.saveError'))
    } finally {
      setSaving(false)
    }
  }

  const handleOpenLogsDir = async () => {
    try {
      await storageApi.openLogsDirectory()
    } catch (error) {
      log.error({ err: error }, 'Failed to open logs directory')
    }
  }

  const handleExportLogs = async () => {
    try {
      setExportingLogs(true)
      const result = await exportLogs(24)
      setExportPath(result.path)
      toast.success(t('settings.sections.general.logs.export.success'))
      // Reveal the exported archive in the file manager so the user can find
      // it immediately. Failure here is non-fatal: the export already
      // succeeded and the path is shown in the UI.
      try {
        await storageApi.revealPath(result.path)
      } catch (revealError) {
        log.warn({ err: revealError }, 'Failed to reveal exported log archive')
      }
    } catch (error) {
      log.error({ err: error }, 'Failed to export logs')
      toast.error(t('settings.sections.general.logs.export.error'))
    } finally {
      setExportingLogs(false)
    }
  }

  const handleCopyExportPath = async () => {
    if (!exportPath) return
    try {
      await navigator.clipboard.writeText(exportPath)
      toast.success(t('settings.sections.general.logs.export.copySuccess'))
    } catch (error) {
      log.warn({ err: error }, 'Failed to copy log export path')
      toast.error(t('settings.sections.general.logs.export.copyError'))
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
          label={t('settings.sections.general.logs.debug.label')}
          description={t('settings.sections.general.logs.debug.description')}
        >
          <Switch
            aria-label={t('settings.sections.general.logs.debug.label')}
            checked={debugMode}
            onCheckedChange={handleDebugModeChange}
            disabled={isBusy}
          />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.general.logs.export.label')}
          description={t('settings.sections.general.logs.export.description')}
        >
          <div className="flex flex-col items-end gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={handleExportLogs}
              disabled={isBusy || exportingLogs}
            >
              {exportingLogs
                ? t('settings.sections.general.logs.export.exporting')
                : t('settings.sections.general.logs.export.button')}
            </Button>
            {exportPath && (
              <div className="flex max-w-96 items-center gap-2 text-xs text-muted-foreground">
                <span className="truncate">{exportPath}</span>
                <Button variant="ghost" size="sm" onClick={handleCopyExportPath}>
                  {t('settings.sections.general.logs.export.copyPath')}
                </Button>
              </div>
            )}
          </div>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.general.logsDirectory.label')}
          description={t('settings.sections.general.logsDirectory.description')}
        >
          <Button variant="outline" size="sm" onClick={handleOpenLogsDir}>
            {t('settings.sections.general.logsDirectory.button')}
          </Button>
        </SettingRow>
      </SettingGroup>

      <AlertDialog open={debugConfirmOpen} onOpenChange={setDebugConfirmOpen}>
        <AlertDialogContent className="bg-card text-card-foreground">
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t('settings.sections.general.logs.debug.confirmTitle')}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('settings.sections.general.logs.debug.confirmDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={saving}>
              {t('settings.sections.general.logs.debug.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction disabled={saving} onClick={handleConfirmDebugMode}>
              {t('settings.sections.general.logs.debug.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
