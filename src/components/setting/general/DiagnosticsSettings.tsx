import { Loader2 } from 'lucide-react'
import { useEffect, useState } from 'react'
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
  Switch,
  Button,
} from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useSavingState } from './useSavingState'

const log = createLogger('general-section')

/**
 * Debug-mode confirmation dialog as a small state machine:
 * - `closed`     — no dialog
 * - `confirming` — asking the user to confirm enabling debug mode
 * - `restarting` — forced, non-dismissable state while the daemon and GUI
 *   restart to pick up the new log profile
 */
type DebugDialogState = 'closed' | 'confirming' | 'restarting'

const handleOpenLogsDir = async () => {
  try {
    await storageApi.openLogsDirectory()
  } catch (error) {
    log.error({ err: error }, 'Failed to open logs directory')
  }
}

export function DiagnosticsSettings() {
  const { t } = useTranslation()
  const { setting, loading, reloadSetting } = useSetting()
  const { saving, runSave } = useSavingState()
  const [debugMode, setDebugMode] = useState(setting?.general.debugMode ?? false)
  const [debugDialog, setDebugDialog] = useState<DebugDialogState>('closed')
  const [exportPath, setExportPath] = useState<string | null>(null)
  const [exportingLogs, setExportingLogs] = useState(false)
  const isBusy = loading || saving
  const isRestarting = debugDialog === 'restarting'

  useEffect(() => {
    if (!setting?.general) return
    setDebugMode(setting.general.debugMode ?? false)
  }, [setting])

  const persistDebugModeOff = () =>
    runSave(
      'Failed to change debug mode',
      async () => {
        const result = await updateDebugMode(false)
        await reloadSetting()
        setDebugMode(result.debugMode)
        if (result.restartRequired) {
          toast.message(t('settings.sections.general.logs.debug.restartToast'))
        }
      },
      'settings.sections.general.logs.debug.error'
    )

  const handleDebugModeChange = (checked: boolean) => {
    if (checked) {
      setDebugDialog('confirming')
    } else {
      void persistDebugModeOff()
    }
  }

  const handleConfirmDebugMode = async () => {
    // Keep the dialog open and switch it into a forced "restarting" state so the
    // user cannot dismiss it while the app and daemon are coming back up.
    setDebugDialog('restarting')
    try {
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
      // Restart failed: drop back to the confirm state so the user can dismiss.
      setDebugDialog('confirming')
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

      <AlertDialog
        open={debugDialog !== 'closed'}
        onOpenChange={open => {
          // While restarting, the dialog is forced: ignore every close request
          // (Escape, overlay, programmatic) until the restart finishes or fails.
          if (isRestarting) return
          if (!open) setDebugDialog('closed')
        }}
      >
        <AlertDialogContent
          className="bg-card text-card-foreground"
          onEscapeKeyDown={event => {
            if (isRestarting) event.preventDefault()
          }}
        >
          <AlertDialogHeader>
            <AlertDialogTitle>
              {isRestarting
                ? t('settings.sections.general.logs.debug.restartingTitle')
                : t('settings.sections.general.logs.debug.confirmTitle')}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {isRestarting
                ? t('settings.sections.general.logs.debug.restartingDescription')
                : t('settings.sections.general.logs.debug.confirmDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {isRestarting ? (
            <AlertDialogFooter>
              <div className="flex w-full items-center justify-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                {t('settings.sections.general.logs.debug.restartingTitle')}
              </div>
            </AlertDialogFooter>
          ) : (
            <AlertDialogFooter>
              <AlertDialogCancel>
                {t('settings.sections.general.logs.debug.cancel')}
              </AlertDialogCancel>
              <AlertDialogAction
                onClick={event => {
                  // Prevent Radix from auto-closing the dialog on action click;
                  // we keep it open to show the forced restarting state.
                  event.preventDefault()
                  void handleConfirmDebugMode()
                }}
              >
                {t('settings.sections.general.logs.debug.confirm')}
              </AlertDialogAction>
            </AlertDialogFooter>
          )}
        </AlertDialogContent>
      </AlertDialog>
    </SettingGroup>
  )
}
