import { Loader2 } from 'lucide-react'
import { useEffect, useEffectEvent, useReducer, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import {
  DEFAULT_MOBILE_LAN_PORT,
  getMobileSyncSettings,
  updateMobileSyncSettings,
  type MobileSyncSettingsView,
  type RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import MobileSyncRegistrationForm from '@/components/device/MobileSyncRegistrationForm'
import { Button } from '@/components/ui/button'
import { createLogger } from '@/lib/logger'

const log = createLogger('mobile-regular-sync-flow')
type State =
  | { kind: 'loading' }
  | { kind: 'failed'; message: string }
  | { kind: 'ready'; settings: MobileSyncSettingsView }

interface Props {
  footerContainer: HTMLDivElement | null
  onCancel: () => void
  onBusyChange: (busy: boolean) => void
  onSuccess: (result: RegisterMobileDeviceResult) => void
  onSettingsChange: (settings: MobileSyncSettingsView) => void
  onConfigure: () => void
}

export default function MobileRegularSyncFlow({
  onCancel,
  onBusyChange,
  onSuccess,
  onSettingsChange,
  onConfigure,
  footerContainer,
}: Props) {
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

  if (state.kind === 'loading') {
    return (
      <output className="flex min-h-40 items-center justify-center gap-2 text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        {t('common.loading')}
      </output>
    )
  }
  const error =
    state.kind === 'failed'
      ? state.message
      : state.settings.lanListenerError
        ? t('devices.mobileSync.statusBar.bindFailed', {
            reason: state.settings.lanListenerError,
          })
        : null
  if (error) {
    return (
      <div className="flex flex-col gap-4">
        <p role="alert" className="text-ui-body text-destructive">
          {error}
        </p>
        {footerContainer &&
          createPortal(
            <>
              <Button variant="outline" onClick={onConfigure}>
                {t('devices.mobileSync.configure')}
              </Button>
              <Button
                onClick={() => {
                  setState({ kind: 'loading' })
                  retry()
                }}
              >
                {t('devices.list.actions.retry')}
              </Button>
            </>,
            footerContainer
          )}
      </div>
    )
  }
  if (state.kind !== 'ready') return null
  if (!state.settings.enabled || !state.settings.lanListenEnabled) {
    return (
      <div className="flex flex-col gap-4">
        <p className="text-ui-body text-muted-foreground">
          {t('devices.mobileSync.enableConfirm.body', {
            port: state.settings.lanPort ?? DEFAULT_MOBILE_LAN_PORT,
          })}
        </p>
        {footerContainer &&
          createPortal(
            <>
              <Button variant="ghost" onClick={onCancel} disabled={enabling}>
                {t('devices.mobileSync.enableConfirm.cancel')}
              </Button>
              <Button onClick={() => void enable()} disabled={enabling}>
                {enabling && <Loader2 className="size-4 animate-spin" />}
                {t(
                  enabling
                    ? 'devices.mobileSync.enableConfirm.enabling'
                    : 'devices.mobileSync.enableConfirm.confirm'
                )}
              </Button>
            </>,
            footerContainer
          )}
      </div>
    )
  }
  return (
    <MobileSyncRegistrationForm
      footerContainer={footerContainer}
      onCancel={onCancel}
      onBusyChange={onBusyChange}
      onSuccess={onSuccess}
    />
  )
}
