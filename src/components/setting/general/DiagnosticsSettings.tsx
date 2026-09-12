import { Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
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
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { openLogsDirectory, useDiagnosticsSettings } from './useDiagnosticsSettings'

export function DiagnosticsSettings() {
  const { t } = useTranslation()
  const {
    captureBusy,
    captureDescription,
    captureStatus,
    captureUnavailable,
    debugDialog,
    debugMode,
    detailedCapture,
    exportingLogs,
    exportPath,
    handleConfirmDebugMode,
    handleCopyExportPath,
    handleDebugModeChange,
    handleDetailedCaptureChange,
    handleExportLogs,
    isBusy,
    setDebugDialog,
  } = useDiagnosticsSettings()
  const isRestarting = debugDialog === 'restarting'

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
        label={t('settings.sections.general.logs.capture.label')}
        description={captureDescription}
      >
        <TooltipProvider>
          <Tooltip disabled={!detailedCapture || captureUnavailable}>
            <TooltipTrigger
              render={
                <Switch
                  aria-label={t('settings.sections.general.logs.capture.label')}
                  checked={detailedCapture}
                  onCheckedChange={checked => void handleDetailedCaptureChange(checked)}
                  disabled={isBusy || captureBusy || captureUnavailable || !captureStatus}
                />
              }
            />
            <TooltipContent role="tooltip" sideOffset={6}>
              {t('settings.sections.general.logs.capture.tooltip', {
                minutes: Math.max(1, Math.ceil((captureStatus?.capture.remainingMs ?? 0) / 60_000)),
              })}
            </TooltipContent>
          </Tooltip>
        </TooltipProvider>
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
        <Button variant="outline" size="sm" onClick={openLogsDirectory}>
          {t('settings.sections.general.logsDirectory.button')}
        </Button>
      </SettingRow>

      <AlertDialog
        open={debugDialog !== 'closed'}
        onOpenChange={(open, eventDetails) => {
          // While restarting, the dialog is forced: ignore every close request
          // (Escape, overlay, programmatic) until the restart finishes or fails.
          if (isRestarting) {
            eventDetails.cancel()
            return
          }
          if (!open) setDebugDialog('closed')
        }}
      >
        <AlertDialogContent className="bg-card text-card-foreground">
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
                  // Prevent the alert dialog from closing on action click;
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
