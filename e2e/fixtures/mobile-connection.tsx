import { LazyMotion, domMax } from 'framer-motion'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import ConnectMobileDialog from '@/components/device/ConnectMobileDialog'
import DeviceListFooter from '@/components/device/DeviceListFooter'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import i18n from '@/i18n'
import { installSettingsFixtureEnvironment } from './settings-environment'
import './typography.css'

installSettingsFixtureEnvironment()
const params = new URLSearchParams(location.search)
await i18n.changeLanguage(params.get('language') || 'zh-CN')
document.documentElement.classList.toggle('dark', params.get('theme') === 'dark')
let enabled = params.get('state') !== 'disabled'
const originalFetch = window.fetch.bind(window)
window.fetch = async (input, options) => {
  const request =
    input instanceof Request ? input : new Request(new URL(String(input), location.href), options)
  const path = new URL(request.url).pathname.replace('/fixture-daemon', '')
  const respond = (data: unknown) => Response.json({ data, ts: Date.now() })
  if (path === '/mobile-sync/settings') {
    if (request.method === 'PATCH') {
      enabled = true
      return respond({ enabled: true, lanListenEnabled: true, lanListenerBindError: null })
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
    return respond({ deviceId: 'fixture-phone', label: (await request.json()).label })
  }
  if (path === '/v2/setup/state')
    return respond({ hasCompleted: true, currentInvitation: null, rePairingRequired: false })
  if (path === '/v2/setup/issue-invitation')
    return respond({ code: 'ABCD1234', expiresAtMs: Date.now() + 300000 })
  if (path === '/v2/setup/cancel') return respond(null)
  if (path === '/member/device-group-choices')
    return respond({
      deviceTrust: {
        localDeviceId: 'local',
        devices: [{ deviceId: 'local', membership: 'active' }],
      },
      issues: [],
    })
  return originalFetch(input, options)
}

export default function Fixture() {
  const [open, setOpen] = useState(false)
  return (
    <LazyMotion features={domMax} strict>
      <VisualEffectsProvider>
        <div className="min-h-screen bg-background text-foreground">
          <aside className="border-r border-border" style={{ width: '16rem' }}>
            <div className="p-4 text-ui-section">{i18n.t('devices.connectMobile.title')}</div>
            <div className="h-48 p-4 text-ui-body">MacBook Pro</div>
            <DeviceListFooter
              onlineCount={1}
              onAddDevice={() => {}}
              onSwitchSpace={() => {}}
              onAddMobile={() => setOpen(true)}
            />
          </aside>
          <ConnectMobileDialog
            open={open}
            onOpenChange={setOpen}
            onDirectSuccess={() => {}}
            onMobileSuccess={() => {
              document.body.dataset.registered = 'true'
            }}
            onSettingsChange={() => {}}
            onConfigure={() => {
              document.body.dataset.configure = 'true'
              setOpen(false)
            }}
          />
        </div>
      </VisualEffectsProvider>
    </LazyMotion>
  )
}
createRoot(document.getElementById('root')!).render(<Fixture />)
