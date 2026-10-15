import { useCallback, useEffect, useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import {
  exportLogs,
  getDiagnosticCaptureStatus,
  getNetworkLogStatus,
  startDiagnosticCapture,
  startNetworkCapture,
  stopDiagnosticCapture,
  stopNetworkCapture,
  updateDebugMode,
  type DiagnosticCaptureStatus,
  type NetworkLogStatus,
} from '@/api/daemon/diagnostics'
import * as storageApi from '@/api/storage'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import {
  networkExportIncomplete,
  networkExportNotice,
  normalizeOfflineExportResult,
  parseExportFailureCause,
  sourceLabelKey,
  type ExportFailureCause,
  type MissingExportSource,
  type NetworkExportNotice,
} from './diagnosticExportFallback'
import { useSavingState } from './useSavingState'

const log = createLogger('general-section')

/** Length of one network capture window. A pending product default; the Engine accepts 1 to 900 seconds. */
const NETWORK_CAPTURE_SECONDS = 300

type DebugDialogState = 'closed' | 'confirming' | 'restarting'

type ExportFallbackNotice = {
  cause?: ExportFailureCause
  missingSources: MissingExportSource[]
}

/** Decimal counter strings from the daemon; a value that does not parse counts as unknown, not zero. */
function counter(value: string | undefined): bigint | null {
  if (value === undefined) return null
  try {
    return BigInt(value)
  } catch {
    return null
  }
}

function mebibytes(bytes: string): string {
  const value = counter(bytes)
  if (value === null) return '?'
  return (Number(value) / (1024 * 1024)).toFixed(value % 1048576n === 0n ? 0 : 1)
}

type DiagnosticsState = {
  debugDialog: DebugDialogState
  exportPath: string | null
  exportFallback: ExportFallbackNotice | null
  exportingLogs: boolean
  captureStatus: DiagnosticCaptureStatus | null
  captureUnavailable: boolean
  captureBusy: boolean
  networkStatus: NetworkLogStatus | null
  networkUnavailable: boolean
  networkBusy: boolean
  networkExportNotice: NetworkExportNotice | null
}

type DiagnosticsAction =
  | { type: 'setDebugDialog'; value: DebugDialogState }
  | { type: 'setExportPath'; value: string }
  | { type: 'setExportFallback'; value: ExportFallbackNotice | null }
  | { type: 'setExportingLogs'; value: boolean }
  | { type: 'captureLoaded'; status: DiagnosticCaptureStatus }
  | { type: 'captureUnavailable' }
  | { type: 'setCaptureBusy'; value: boolean }
  | { type: 'networkLoaded'; status: NetworkLogStatus }
  | { type: 'networkUnavailable' }
  | { type: 'setNetworkBusy'; value: boolean }
  | { type: 'setNetworkExportNotice'; value: NetworkExportNotice | null }

const initialState: DiagnosticsState = {
  debugDialog: 'closed',
  exportPath: null,
  exportFallback: null,
  exportingLogs: false,
  captureStatus: null,
  captureUnavailable: false,
  captureBusy: false,
  networkStatus: null,
  networkUnavailable: false,
  networkBusy: false,
  networkExportNotice: null,
}

function diagnosticsReducer(state: DiagnosticsState, action: DiagnosticsAction): DiagnosticsState {
  switch (action.type) {
    case 'setDebugDialog':
      return { ...state, debugDialog: action.value }
    case 'setExportPath':
      return { ...state, exportPath: action.value }
    case 'setExportFallback':
      return { ...state, exportFallback: action.value }
    case 'setExportingLogs':
      return { ...state, exportingLogs: action.value }
    case 'captureLoaded':
      return { ...state, captureStatus: action.status, captureUnavailable: false }
    case 'captureUnavailable':
      return { ...state, captureUnavailable: true }
    case 'setCaptureBusy':
      return { ...state, captureBusy: action.value }
    case 'networkLoaded':
      return { ...state, networkStatus: action.status, networkUnavailable: false }
    case 'networkUnavailable':
      return { ...state, networkUnavailable: true }
    case 'setNetworkBusy':
      return { ...state, networkBusy: action.value }
    case 'setNetworkExportNotice':
      return { ...state, networkExportNotice: action.value }
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

  const networkDetailed = state.networkStatus?.mode === 'detailed'

  const loadNetworkStatus = useCallback(async () => {
    try {
      const status = await getNetworkLogStatus()
      dispatch({ type: 'networkLoaded', status })
      return status
    } catch (error) {
      log.warn({ err: error }, 'Failed to read network log status')
      dispatch({ type: 'networkUnavailable' })
      return null
    }
  }, [])

  useEffect(() => {
    void loadNetworkStatus()
  }, [loadNetworkStatus])

  useEffect(() => {
    if (!networkDetailed && !state.networkUnavailable) return
    const timer = window.setInterval(() => void loadNetworkStatus(), 5_000)
    return () => window.clearInterval(timer)
  }, [networkDetailed, loadNetworkStatus, state.networkUnavailable])

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
    dispatch({ type: 'setExportFallback', value: null })
    dispatch({ type: 'setNetworkExportNotice', value: null })
    try {
      let path: string | null
      let fallback: ExportFallbackNotice | null = null
      let exportNotice: 'complete' | 'partial' | 'offline' = 'complete'
      let networkNotice: NetworkExportNotice | null = null
      try {
        const result = await exportLogs(24)
        path = result.path
        networkNotice = networkExportNotice(
          result.collection.network.outcome,
          result.collection.network.reasons
        )
        if (
          result.enginePreparation.flush !== 'completed' ||
          result.collection.unreadableFiles.length > 0 ||
          result.collection.truncatedFiles.length > 0 ||
          networkExportIncomplete(networkNotice)
        ) {
          exportNotice = 'partial'
        }
      } catch (daemonError) {
        log.warn({ err: daemonError }, 'Daemon export unavailable; using retained logs')
        const offline = normalizeOfflineExportResult(await commands.exportStartupLogs())
        path = offline.path
        if (!path) return
        networkNotice = offline.network
        exportNotice = 'offline'
        fallback = {
          cause: parseExportFailureCause(daemonError),
          missingSources: offline.missingSources,
        }
      }
      if (!path) return
      dispatch({ type: 'setExportPath', value: path })
      dispatch({ type: 'setExportFallback', value: fallback })
      dispatch({ type: 'setNetworkExportNotice', value: networkNotice })
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

  const handleNetworkCaptureChange = async (checked: boolean) => {
    dispatch({ type: 'setNetworkBusy', value: true })
    try {
      if (checked) {
        const started = await startNetworkCapture(NETWORK_CAPTURE_SECONDS)
        dispatch({ type: 'networkLoaded', status: started.status })
      } else {
        const captureId = state.networkStatus?.captureId
        if (captureId) {
          dispatch({ type: 'networkLoaded', status: await stopNetworkCapture(captureId) })
        }
      }
    } catch (error) {
      const recovered = await loadNetworkStatus()
      const reachedRequestedState = recovered?.mode === (checked ? 'detailed' : 'standard')
      if (!reachedRequestedState) {
        log.error({ err: error }, 'Failed to change network capture state')
        toast.error(t('settings.sections.general.logs.network.error'))
      }
    } finally {
      dispatch({ type: 'setNetworkBusy', value: false })
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

  const exportFallbackLines: string[] = []
  if (state.exportFallback?.cause) {
    exportFallbackLines.push(
      t(`settings.sections.general.logs.export.cause.${state.exportFallback.cause}`)
    )
  }
  if (state.exportFallback && state.exportFallback.missingSources.length > 0) {
    const sources = state.exportFallback.missingSources.map(({ source }) => {
      const key = sourceLabelKey(source)
      return key ? t(`settings.sections.general.logs.export.sources.${key}`) : source
    })
    exportFallbackLines.push(
      t('settings.sections.general.logs.export.missingSources', { sources: sources.join(', ') })
    )
  }

  const networkExportLines: string[] = []
  if (state.networkExportNotice) {
    const { outcome, reasons } = state.networkExportNotice
    networkExportLines.push(
      t(`settings.sections.general.logs.export.network.outcome.${outcome}`),
      ...reasons.map(reason =>
        t(`settings.sections.general.logs.export.network.reason.${reason}`, {
          defaultValue: t('settings.sections.general.logs.export.network.reason.unrecognized'),
        })
      )
    )
  }

  const networkLines: string[] = []
  const networkStatus = state.networkStatus
  if (networkStatus && networkStatus.availability === 'configured') {
    const counters = networkStatus.counters
    const lost =
      counters &&
      [
        counters.queueDropped,
        counters.rateLimited,
        counters.oversized,
        counters.closedDropped,
        counters.writeFailed,
      ].reduce<bigint | null>((total, value) => {
        const parsed = counter(value)
        return total === null || parsed === null ? null : total + parsed
      }, 0n)
    if (counters) {
      networkLines.push(
        t('settings.sections.general.logs.network.counters', {
          written: counters.written,
          dropped: lost === null || lost === undefined ? '?' : lost.toString(),
          failed: counters.writeFailed,
        })
      )
    }
    if (networkStatus.budget) {
      const retentionSeconds = Number(networkStatus.budget.retentionSeconds)
      const retention =
        retentionSeconds >= 86_400
          ? t('settings.sections.general.logs.network.units.day', {
              count: Math.round(retentionSeconds / 86_400),
            })
          : retentionSeconds >= 3_600
            ? t('settings.sections.general.logs.network.units.hour', {
                count: Math.round(retentionSeconds / 3_600),
              })
            : t('settings.sections.general.logs.network.units.minute', {
                count: Math.max(1, Math.round(retentionSeconds / 60)),
              })
      networkLines.push(
        t('settings.sections.general.logs.network.budget', {
          run: mebibytes(networkStatus.budget.runBytes),
          root: mebibytes(networkStatus.budget.rootBytes),
          retention,
        })
      )
    }
    networkLines.push(t('settings.sections.general.logs.network.coverage'))
  }
  const networkConfigured = networkStatus?.availability === 'configured'

  return {
    ...state,
    exportFallbackLines,
    networkExportLines,
    networkLines,
    networkDetailed,
    networkDescription: state.networkUnavailable
      ? t('settings.sections.general.logs.network.unavailable')
      : !networkStatus
        ? t('settings.sections.general.logs.network.loading')
        : !networkConfigured
          ? t('settings.sections.general.logs.network.notConfigured')
          : networkStatus.state !== 'ready'
            ? t(`settings.sections.general.logs.network.state.${networkStatus.state ?? 'starting'}`)
            : !networkDetailed
              ? t('settings.sections.general.logs.network.standard', {
                  minutes: NETWORK_CAPTURE_SECONDS / 60,
                })
              : t('settings.sections.general.logs.network.active', {
                  minutes: Math.max(
                    1,
                    Math.ceil((networkStatus.detailedRemainingMs ?? 0) / 60_000)
                  ),
                }),
    networkSwitchDisabled:
      state.networkUnavailable ||
      !networkStatus ||
      !networkConfigured ||
      networkStatus.state !== 'ready',
    handleNetworkCaptureChange,
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
