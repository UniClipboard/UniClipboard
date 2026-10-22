import { useTranslation } from 'react-i18next'
import {
  DEFAULT_MOBILE_LAN_PORT,
  type MobileSyncSettingsView,
  type RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { useMobileSyncSetup } from '@/hooks/useMobileSyncSetup'
import { Button, InlineNotice, ProgressBar, TaskPanel, Text } from '@/ui-kit'
import { DevicesKitPhoneRegistration } from './DevicesKitPhoneRegistration'

export function DevicesKitPhoneSetup(props: {
  onBack: () => void
  onSuccess: (result: RegisterMobileDeviceResult) => void
  onBusyChange: (busy: boolean) => void
  onSettingsChange: (settings: MobileSyncSettingsView) => void
  onConfigure: () => void
}) {
  const { t } = useTranslation()
  const { state, enabling, enable, reload, readyToRegister, error } = useMobileSyncSetup(props)
  if (readyToRegister) {
    return <DevicesKitPhoneRegistration {...props} />
  }
  return (
    <TaskPanel
      title={t(error ? 'devices.tasks.connectionUnavailable' : 'devices.tasks.enableTitle')}
      description={t('devices.connectMobile.regularDescription')}
      stage={t('devices.tasks.step', { current: 2, total: 2 })}
      back={{ label: t('devices.tasks.back'), onClick: props.onBack, disabled: enabling }}
      actions={
        error ? (
          <>
            <Button onClick={props.onConfigure}>{t('devices.mobileSync.configure')}</Button>
            <Button variant="primary" onClick={reload}>
              {t('devices.list.actions.retry')}
            </Button>
          </>
        ) : state.kind === 'ready' ? (
          <Button variant="primary" busy={enabling} onClick={() => void enable()}>
            {t('devices.mobileSync.enableConfirm.confirm')}
          </Button>
        ) : undefined
      }
    >
      {state.kind === 'loading' ? (
        <ProgressBar label={t('common.loading')} />
      ) : error ? (
        <InlineNotice tone="error" title={error} />
      ) : (
        <Text>
          {t('devices.mobileSync.enableConfirm.body', {
            port:
              state.kind === 'ready'
                ? (state.settings.lanPort ?? DEFAULT_MOBILE_LAN_PORT)
                : DEFAULT_MOBILE_LAN_PORT,
          })}
        </Text>
      )}
    </TaskPanel>
  )
}
