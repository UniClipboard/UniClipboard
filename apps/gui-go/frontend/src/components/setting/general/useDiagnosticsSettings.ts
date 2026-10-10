import { useCallback, useEffect, useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import {
  exportLogs,
  getDiagnosticCaptureStatus,
  startDiagnosticCapture,
  stopDiagnosticCapture,
  updateDebugMode,
  type DiagnosticCaptureStatus,
} from '@/api/daemon/diagnostics'
import * as storageApi from '@/api/storage'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { useSavingState } from './useSavingState'

const log = createLogger('general-section')

type DebugDialogState = 'closed' | 'confirming' | 'restarting'

type DiagnosticsState = {
  debugDialog: DebugDialogState
  exportPath: string | null
  exportingLogs: boolean
  captureStatus: DiagnosticCaptureStatus | null
  captureUnavailable: boolean
  captureBusy: boolean
}

type DiagnosticsAction =
  | { type: 'setDebugDialog'; value: DebugDialogState }
  | { type: 'setExportPath'; value: string }
  | { type: 'setExportingLogs'; value: boolean }
  | { type: 'captureLoaded'; status: DiagnosticCaptureStatus }
  | { type: 'captureUnavailable' }
  | { type: 'setCaptureBusy'; value: boolean }

const initialState: DiagnosticsState = {
  debugDialog: 'closed',
  exportPath: null,
  exportingLogs: false,
  captureStatus: null,
  captureUnavailable: false,
  captureBusy: false,
}

function diagnosticsReducer(state: DiagnosticsState, action: DiagnosticsAction): DiagnosticsState {
  switch (action.type) {
    case 'setDebugDialog':
      return { ...state, debugDialog: action.value }
    case 'setExportPath':
      return { ...state, exportPath: action.value }
    case 'setExportingLogs':
      return { ...state, exportingLogs: action.value }
    case 'captureLoaded':
      return { ...state, captureStatus: action.status, captureUnavailable: false }
    case 'captureUnavailable':
      return { ...state, captureUnavailable: true }
    case 'setCaptureBusy':
      return { ...state, captureBusy: action.value }
  }
}

export function useDiagnosticsSettings() {
  const { t } = useTranslation()
  const { setting, loading, reloadSetting } = useSetting()
  const { saving, runSave } = useSavingState()
  const [state, dispatch] = useReducer(diagnosticsReducer, initialState)
  const detailedCapture = state.captureStatus?.capture.mode === 'detailed'

  const loadCaptureStatus = useCallback(async () => {
    try {
      const status = await getDiagnosticCaptureStatus()
      dispatch({ type: 'captureLoaded', status })
      return status
    } catch (error) {
      log.warn({ err: error }, 'Failed to read detailed capture status')
      dispatch({ type: 'captureUnavailable' })
      return null
    }
  }, [])

  useEffect(() => {
    void loadCaptureStatus()
  }, [loadCaptureStatus])

  useEffect(() => {
    if (!detailedCapture && !state.captureUnavailable) return
    const timer = window.setInterval(() => void loadCaptureStatus(), 5_000)
    return () => window.clearInterval(timer)
  }, [detailedCapture, loadCaptureStatus, state.captureUnavailable])

  const handleDebugModeChange = (checked: boolean) => {
    if (checked) {
      dispatch({ type: 'setDebugDialog', value: 'confirming' })
      return
    }
    void runSave(
      'Failed to change debug mode',
      async () => {
        const result = await updateDebugMode(false)
        await reloadSetting()
        if (result.restartRequired) {
          toast.message(t('settings.sections.general.logs.debug.restartToast'))
        }
      },
      'settings.sections.general.logs.debug.error'
    )
  }

  const handleConfirmDebugMode = async () => {
    dispatch({ type: 'setDebugDialog', value: 'restarting' })
    try {
      await updateDebugMode(true)
      await commands.restartDaemon()
      await commands.restartApp()
    } catch (error) {
      log.error({ err: error }, 'Failed to enable debug mode and restart')
      toast.error(t('settings.sections.general.logs.debug.error'))
      await reloadSetting().catch(reloadError => {
        log.warn({ err: reloadError }, 'Failed to reload debug mode after restart failure')
      })
      dispatch({ type: 'setDebugDialog', value: 'confirming' })
    }
  }

  const handleExportLogs = async () => {
    dispatch({ type: 'setExportingLogs', value: true })
    try {
      let path: string | null
      let exportNotice: 'complete' | 'partial' | 'offline' = 'complete'
      try {
        const result = await exportLogs(24)
        path = result.path
        if (
          result.enginePreparation.flush !== 'completed' ||
          result.collection.unreadableFiles.length > 0 ||
          result.collection.truncatedFiles.length > 0
        ) {
          exportNotice = 'partial'
        }
      } catch (daemonError) {
        log.warn({ err: daemonError }, 'Daemon export unavailable; using retained logs')
        path = await commands.exportStartupLogs()
        if (!path) return
        exportNotice = 'offline'
      }
      if (!path) return
      dispatch({ type: 'setExportPath', value: path })
      if (exportNotice === 'complete') {
        toast.success(t('settings.sections.general.logs.export.success'))
      } else {
        toast.message(
          t(
            exportNotice === 'offline'
              ? 'settings.sections.general.logs.export.offlineSuccess'
              : 'settings.sections.general.logs.export.partialSuccess'
          )
        )
      }
      try {
        await storageApi.revealPath(path)
      } catch (revealError) {
        log.warn({ err: revealError }, 'Failed to reveal exported log archive')
      }
    } catch (error) {
      log.error({ err: error }, 'Failed to export logs')
      toast.error(t('settings.sections.general.logs.export.error'))
    } finally {
      dispatch({ type: 'setExportingLogs', value: false })
    }
  }

  const handleDetailedCaptureChange = async (checked: boolean) => {
    dispatch({ type: 'setCaptureBusy', value: true })
    try {
      if (checked) {
        const status = await startDiagnosticCapture(600)
        dispatch({ type: 'captureLoaded', status })
      } else if (state.captureStatus?.capture.captureId) {
        await stopDiagnosticCapture(state.captureStatus.capture.captureId)
        await loadCaptureStatus()
      }
    } catch (error) {
      const recovered = await loadCaptureStatus()
      const reachedRequestedState = recovered?.capture.mode === (checked ? 'detailed' : 'standard')
      if (!reachedRequestedState) {
        log.error({ err: error }, 'Failed to change detailed capture state')
        toast.error(t('settings.sections.general.logs.capture.error'))
      }
    } finally {
      dispatch({ type: 'setCaptureBusy', value: false })
    }
  }

  const handleCopyExportPath = async () => {
    if (!state.exportPath) return
    try {
      await navigator.clipboard.writeText(state.exportPath)
      toast.success(t('settings.sections.general.logs.export.copySuccess'))
    } catch (error) {
      log.warn({ err: error }, 'Failed to copy log export path')
      toast.error(t('settings.sections.general.logs.export.copyError'))
    }
  }

  return {
    ...state,
    captureDescription: state.captureUnavailable
      ? t('settings.sections.general.logs.capture.unavailable')
      : !state.captureStatus
        ? t('settings.sections.general.logs.capture.loading')
        : !detailedCapture
          ? t('settings.sections.general.logs.capture.standard')
          : t('settings.sections.general.logs.capture.active', {
              minutes: Math.max(1, Math.ceil(state.captureStatus.capture.remainingMs / 60_000)),
            }),
    debugMode: setting?.general.debugMode ?? false,
    detailedCapture,
    isBusy: loading || saving,
    setDebugDialog: (value: DebugDialogState) => dispatch({ type: 'setDebugDialog', value }),
    handleDebugModeChange,
    handleConfirmDebugMode,
    handleExportLogs,
    handleDetailedCaptureChange,
    handleCopyExportPath,
  }
}

export async function openLogsDirectory() {
  try {
    await storageApi.openLogsDirectory()
  } catch (error) {
    log.error({ err: error }, 'Failed to open logs directory')
  }
}
