import { configureStore } from '@reduxjs/toolkit'
import { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import { Provider } from 'react-redux'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import { useSwitchSpace } from '@/hooks/useSwitchSpace'
import i18n from '@/i18n'
import { DevicesKitConnectPhone } from '@/pages/devices-kit/DevicesKitConnectPhone'
import { DevicesKitInvitationDetail } from '@/pages/devices-kit/DevicesKitInvitationDetail'
import { DevicesKitPhoneComplete } from '@/pages/devices-kit/DevicesKitPhoneComplete'
import { DevicesKitPhoneNetwork } from '@/pages/devices-kit/DevicesKitPhoneNetwork'
import { DevicesKitSwitchSpace } from '@/pages/devices-kit/DevicesKitSwitchSpace'
import { useDeviceInvitationRow } from '@/pages/devices-kit/useDeviceInvitationRow'
import { Button, ChoiceList, Sheet, UiRoot } from '@/ui-kit'
import { installSettingsFixtureEnvironment } from './settings-environment'
import '@/App.css'

// Real forms, state machines and platform controls with scripted native transport and daemon responses.
const store = configureStore({ reducer: (state = {}) => state })
const scheme = new URLSearchParams(location.search).get('scheme') === 'dark' ? 'dark' : 'light'
installSettingsFixtureEnvironment()
const params = new URLSearchParams(location.search)
let enabled = params.get('state') !== 'disabled'
const fallbackFetch = window.fetch.bind(window)
window.fetch = async (input, options) => {
  const request =
    input instanceof Request ? input : new Request(new URL(String(input), location.href), options)
  const path = new URL(request.url).pathname.replace('/fixture-daemon', '')
  const respond = (data: unknown) => Response.json({ data, ts: Date.now() })
  if (path === '/mobile-sync/settings') {
    if (request.method === 'PATCH') {
      enabled = true
      return respond({ lanListenerBindError: null })
    }
    return respond({
      enabled,
      lanListenEnabled: enabled,
      lanPort: 42720,
      shortcutInstallMethods: [],
      lanListenerError: params.get('state') === 'error' ? 'Port unavailable' : null,
    })
  }
  if (path === '/mobile-sync/devices' && request.method === 'POST') {
    Object.assign(window, { __taskRegistration: await request.json() })
    return respond({
      deviceId: 'fixture-phone',
      label: 'Test phone',
      username: 'phoneUser',
      password: 'fixture-password',
      baseUrl: 'http://192.0.2.10:42720',
      connectUri: 'uniclipboard://connect?fixture=1',
    })
  }
  if (path === '/mobile-sync/lan-interfaces')
    return respond([{ name: 'Wi-Fi', ipv4: '192.0.2.10' }])
  if (path === '/v2/setup/state')
    return respond({ hasCompleted: true, currentInvitation: null, rePairingRequired: false })
  if (path === '/v2/setup/issue-invitation')
    return respond({ code: '012345', expiresAtMs: Date.now() + 300000 })
  if (path === '/member/device-group-choices')
    return respond({
      deviceTrust: {
        localDeviceId: 'local',
        devices: [{ deviceId: 'local', membership: 'active' }],
      },
      issues: [],
    })
  return fallbackFetch(input, options)
}

export function SwitchSpaceFixture() {
  const [entered, setEntered] = useState(false)
  useEffect(() => setEntered(true), [])
  const [view, setView] = useState('devices')
  const [busy, setBusy] = useState(false)
  const [credential, setCredential] = useState<RegisterMobileDeviceResult | null>(null)
  const invitation = useDeviceInvitationRow(() => setView('devices'))
  const model = useSwitchSpace({ onOpenChange: () => back(), autoClose: false })
  const back = () => {
    model.reset()
    setCredential(null)
    setView('devices')
  }
  return (
    <UiRoot colorScheme={scheme}>
      <div className="h-screen">
        <Sheet
          variant={view === 'devices' || view === 'direct' ? 'management' : 'task'}
          open={entered}
          title={i18n.t('devices.panel.listTitle')}
          onOpenChange={() => {
            if (!busy && !['migrating', 'processing'].includes(model.step)) back()
          }}
          done={
            view === 'devices'
              ? { label: i18n.t('history.composite.done'), onClick: back }
              : undefined
          }
        >
          {view === 'join' ? (
            <DevicesKitSwitchSpace model={model} onBack={back} />
          ) : view === 'phone' ? (
            <DevicesKitConnectPhone
              onBack={back}
              onDirect={() => {
                invitation.start()
                setView('direct')
              }}
              onConfigure={() => setView('network')}
              onBusyChange={setBusy}
              onSettingsChange={() => {}}
              onSuccess={result => {
                setBusy(false)
                setCredential(result)
                setView('registered')
              }}
            />
          ) : view === 'network' ? (
            <DevicesKitPhoneNetwork
              onBack={() => setView('phone')}
              onBusyChange={setBusy}
              onSettingsChange={() => {}}
            />
          ) : view === 'direct' ? (
            <>
              <DevicesKitInvitationDetail row={invitation} />
              <Button onClick={back}>Back to devices</Button>
            </>
          ) : view === 'registered' && credential ? (
            <DevicesKitPhoneComplete credential={credential} onDone={back} />
          ) : (
            <ChoiceList
              label="Device tasks"
              items={[
                { id: 'join', title: i18n.t('devices.kit.joinSpace'), icon: 'add' },
                { id: 'phone', title: i18n.t('devices.kit.connectPhone'), icon: 'phone' },
              ]}
              onSelect={setView}
            />
          )}
        </Sheet>
      </div>
    </UiRoot>
  )
}

void i18n.changeLanguage(params.get('language') ?? 'zh-CN').then(() => {
  createRoot(document.getElementById('root')!).render(
    <Provider store={store}>
      <I18nextProvider i18n={i18n}>
        <SwitchSpaceFixture />
      </I18nextProvider>
    </Provider>
  )
})
