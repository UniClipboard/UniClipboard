import { useEffect, useEffectEvent, useReducer, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getMobileSyncSettings,
  updateMobileSyncSettings,
  type MobileSyncSettingsView,
} from '@/api/tauri-command/mobile_sync'
import { createLogger } from '@/lib/logger'
const log = createLogger('mobile-sync-setup')
type State =
  | { kind: 'loading' }
  | { kind: 'failed'; message: string }
  | { kind: 'ready'; settings: MobileSyncSettingsView }

export function useMobileSyncSetup({
  onSettingsChange,
  onBusyChange,
}: {
  onSettingsChange: (settings: MobileSyncSettingsView) => void
  onBusyChange: (busy: boolean) => void
}) {
  const { t } = useTranslation()
  const [state, setState] = useState<State>({ kind: 'loading' })
  const [attempt, retry] = useReducer((value: number) => value + 1, 0)
  const [enabling, setEnabling] = useState(false)
  const reportSettings = useEffectEvent(onSettingsChange)
  const reportLoadError = useEffectEvent(() => {
    setState({
      kind: 'failed',
      message: t('devices.connectMobile.loadFailed'),
    })
  })

  useEffect(() => {
    let cancelled = false
    void getMobileSyncSettings()
      .then(settings => {
        if (cancelled) return
        setState({ kind: 'ready', settings })
        reportSettings(settings)
      })
      .catch(() => {
        if (cancelled) return
        log.warn('failed to load mobile sync settings')
        reportLoadError()
      })
    return () => {
      cancelled = true
    }
  }, [attempt])

  const enable = async () => {
    setEnabling(true)
    onBusyChange(true)
    try {
      const result = await updateMobileSyncSettings({
        enabled: true,
        lanListenEnabled: true,
      })
      if (result.lanListenerBindError) {
        setState({
          kind: 'failed',
          message: t('devices.mobileSync.statusBar.bindFailed', {
            reason: result.lanListenerBindError,
          }),
        })
        return
      }
      const settings = await getMobileSyncSettings()
      setState({ kind: 'ready', settings })
      onSettingsChange(settings)
    } catch {
      log.warn('failed to enable mobile sync')
      setState({
        kind: 'failed',
        message: t('devices.connectMobile.enableFailed'),
      })
    } finally {
      setEnabling(false)
      onBusyChange(false)
    }
  }

  return {
    state,
    readyToRegister:
      state.kind === 'ready' &&
      state.settings.enabled &&
      state.settings.lanListenEnabled &&
      !state.settings.lanListenerError,
    error:
      state.kind === 'failed'
        ? state.message
        : state.kind === 'ready'
          ? state.settings.lanListenerError
          : null,
    enabling,
    enable,
    reload: () => {
      setState({ kind: 'loading' })
      retry()
    },
  }
}
