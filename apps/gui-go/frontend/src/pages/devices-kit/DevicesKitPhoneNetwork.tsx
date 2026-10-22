import { useEffect, useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import {
  DEFAULT_MOBILE_LAN_PORT,
  getMobileSyncSettings,
  listMobileLanInterfaces,
  type LanInterfaceView,
  updateMobileSyncSettings,
  type MobileSyncSettingsView,
} from '@/api/tauri-command/mobile_sync'
import { translateMobileSyncError } from '@/components/device/mobile-sync-errors'
import { createLogger } from '@/lib/logger'
import {
  Button,
  FormField,
  InlineNotice,
  ProgressBar,
  SettingsGroup,
  SettingsRow,
  Select,
  Switch,
  TaskPanel,
} from '@/ui-kit'

const log = createLogger('phone-network')

export function DevicesKitPhoneNetwork({
  onBack,
  onSettingsChange,
  onBusyChange,
}: {
  onBack: () => void
  onSettingsChange: (settings: MobileSyncSettingsView) => void
  onBusyChange: (busy: boolean) => void
}) {
  const { t } = useTranslation()
  const [state, update] = useReducer(
    (
      current: {
        loaded: boolean
        available: boolean
        interfaces: LanInterfaceView[]
        ip: string
        enabled: boolean
        lan: boolean
        port: string
        error: string | null
        busy: boolean
        confirm: boolean
      },
      changes: Partial<typeof current>
    ) => ({ ...current, ...changes }),
    {
      loaded: false,
      available: false,
      interfaces: [],
      ip: 'auto',
      enabled: false,
      lan: false,
      port: '',
      error: null,
      busy: false,
      confirm: false,
    }
  )
  useEffect(() => {
    let active = true
    void Promise.all([getMobileSyncSettings(), listMobileLanInterfaces()])
      .then(([view, interfaces]) => {
        if (active)
          update({
            loaded: true,
            available: true,
            interfaces,
            ip: view.lanAdvertiseIp ?? 'auto',
            error: view.lanListenerError ?? null,
            enabled: view.enabled,
            lan: view.lanListenEnabled,
            port: String(view.lanPort ?? DEFAULT_MOBILE_LAN_PORT),
          })
      })
      .catch(error => {
        log.warn({ err: error }, 'Failed to load phone network settings')
        if (active) update({ loaded: true, error: translateMobileSyncError(t, error) })
      })
    return () => {
      active = false
    }
  }, [t])
  const save = async () => {
    const port = Number(state.port)
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      update({ error: t('devices.tasks.invalidPort') })
      return
    }
    if (state.enabled && state.lan && !state.confirm) {
      update({ confirm: true, error: null })
      return
    }
    update({ busy: true, error: null })
    onBusyChange(true)
    try {
      const result = await updateMobileSyncSettings({
        enabled: state.enabled,
        lanListenEnabled: state.lan,
        lanPort: port,
        lanAdvertiseIp: state.ip === 'auto' ? null : state.ip,
      })
      if (result.lanListenerBindError) {
        update({ error: result.lanListenerBindError })
        return
      }
      onSettingsChange(await getMobileSyncSettings())
      onBack()
    } catch (error) {
      log.warn({ err: error }, 'Failed to save phone network settings')
      update({ error: translateMobileSyncError(t, error) })
    } finally {
      update({ busy: false })
      onBusyChange(false)
    }
  }
  return (
    <TaskPanel
      title={t('devices.mobileSync.settingsSheet.title')}
      description={t('devices.mobileSync.settingsSheet.description')}
      back={{
        label: t('devices.tasks.back'),
        onClick: state.confirm ? () => update({ confirm: false }) : onBack,
        disabled: state.busy,
      }}
      actions={
        <Button
          variant="primary"
          busy={state.busy}
          disabled={!state.available}
          onClick={() => void save()}
        >
          {t(
            state.confirm ? 'devices.mobileSync.lanListener.warning.confirm' : 'devices.tasks.save'
          )}
        </Button>
      }
    >
      {!state.loaded ? (
        <ProgressBar label={t('common.loading')} />
      ) : (
        <>
          {state.error && <InlineNotice tone="error" title={state.error} />}
          {state.confirm ? (
            <InlineNotice
              tone="warning"
              title={t('devices.mobileSync.lanListener.warning.title')}
              text={t('devices.mobileSync.lanListener.warning.body')}
            />
          ) : (
            <>
              <SettingsGroup>
                <SettingsRow label={t('devices.mobileSync.enabled.label')}>
                  <Switch
                    label={t('devices.mobileSync.enabled.label')}
                    labelHidden
                    checked={state.enabled}
                    onCheckedChange={enabled => update({ enabled })}
                    disabled={state.busy}
                  />
                </SettingsRow>
                <SettingsRow label={t('devices.mobileSync.lanListener.label')}>
                  <Switch
                    label={t('devices.mobileSync.lanListener.label')}
                    labelHidden
                    checked={state.lan}
                    onCheckedChange={lan => update({ lan })}
                    disabled={!state.enabled || state.busy}
                  />
                </SettingsRow>
              </SettingsGroup>
              <SettingsGroup>
                <SettingsRow label={t('devices.mobileSync.lanListener.bindIp.label')}>
                  <Select
                    label={t('devices.mobileSync.lanListener.bindIp.label')}
                    value={state.ip}
                    onValueChange={ip => update({ ip })}
                    disabled={state.busy}
                    items={[
                      { value: 'auto', label: t('devices.mobileSync.lanListener.bindIp.auto') },
                      ...state.interfaces.map(item => ({
                        value: item.ipv4,
                        label: `${item.name} (${item.ipv4})`,
                      })),
                      ...(state.ip !== 'auto' &&
                      !state.interfaces.some(item => item.ipv4 === state.ip)
                        ? [{ value: state.ip, label: state.ip }]
                        : []),
                    ]}
                  />
                </SettingsRow>
              </SettingsGroup>
              <FormField
                label={t('devices.mobileSync.lanListener.port.label')}
                value={state.port}
                onValueChange={port => update({ port, error: null })}
                disabled={state.busy}
              />
            </>
          )}
        </>
      )}
    </TaskPanel>
  )
}
