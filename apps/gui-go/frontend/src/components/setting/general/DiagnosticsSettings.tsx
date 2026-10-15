import { useTranslation } from 'react-i18next'
import { Button, Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui'
import { SettingAlert, SettingSwitch } from '../SettingControls'
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
    exportFallbackLines,
    exportingLogs,
    exportPath,
    handleConfirmDebugMode,
    handleCopyExportPath,
    handleDebugModeChange,
    handleDetailedCaptureChange,
    handleExportLogs,
    isBusy,
    networkDescription,
    networkExportLines,
    networkLines,
    setDebugDialog,
  } = useDiagnosticsSettings()
  const isRestarting = debugDialog === 'restarting'

  return (
    <SettingGroup title={t('settings.sections.general.logsDirectory.title')}>
      <SettingRow
        label={t('settings.sections.general.logs.debug.label')}
        description={t('settings.sections.general.logs.debug.description')}
      >
        <SettingSwitch
          label={t('settings.sections.general.logs.debug.label')}
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
            <TooltipTrigger render={<span className="inline-flex" />}>
              <SettingSwitch
                label={t('settings.sections.general.logs.capture.label')}
                checked={detailedCapture}
                onCheckedChange={checked => void handleDetailedCaptureChange(checked)}
                disabled={isBusy || captureBusy || captureUnavailable || !captureStatus}
              />
            </TooltipTrigger>
            <TooltipContent role="tooltip" sideOffset={6}>
              {t('settings.sections.general.logs.capture.tooltip', {
                minutes: Math.max(1, Math.ceil((captureStatus?.capture.remainingMs ?? 0) / 60_000)),
              })}
            </TooltipContent>
          </Tooltip>
        </TooltipProvider>
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.logs.network.label')}
        description={networkDescription}
      >
        <div className="flex flex-col items-end gap-2">
          {networkLines.map(line => (
            <p key={line} className="max-w-96 text-right text-ui-caption text-muted-foreground">
              {line}
            </p>
          ))}
        </div>
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
            <div className="flex max-w-96 items-center gap-2 text-ui-caption text-muted-foreground">
              <span className="truncate">{exportPath}</span>
              <Button variant="ghost" size="sm" onClick={handleCopyExportPath}>
                {t('settings.sections.general.logs.export.copyPath')}
              </Button>
            </div>
          )}
          {exportFallbackLines.map(line => (
            <p key={line} className="max-w-96 text-right text-ui-caption text-muted-foreground">
              {line}
            </p>
          ))}
          {networkExportLines.map(line => (
            <p key={line} className="max-w-96 text-right text-ui-caption text-muted-foreground">
              {line}
            </p>
          ))}
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

      <SettingAlert
        open={debugDialog !== 'closed'}
        onOpenChange={open => {
          if (!open) setDebugDialog('closed')
        }}
        title={
          isRestarting
            ? t('settings.sections.general.logs.debug.restartingTitle')
            : t('settings.sections.general.logs.debug.confirmTitle')
        }
        description={
          isRestarting
            ? t('settings.sections.general.logs.debug.restartingDescription')
            : t('settings.sections.general.logs.debug.confirmDescription')
        }
        confirmLabel={t('settings.sections.general.logs.debug.confirm')}
        cancelLabel={t('settings.sections.general.logs.debug.cancel')}
        busyLabel={
          isRestarting ? t('settings.sections.general.logs.debug.restartingTitle') : undefined
        }
        busyReplacesActions
        // The alert stays open to show the forced restarting state.
        onConfirm={async () => {
          await handleConfirmDebugMode()
        }}
      />
    </SettingGroup>
  )
}
