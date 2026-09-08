// Browser-only component fixture. The native policy is tested separately in Rust.
import { LazyMotion, domMax, m } from 'framer-motion'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { visualEffectsApi } from '@/api/visual-effects'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import SmoothModeSetting from '@/components/setting/SmoothModeSetting'
import { Dialog, DialogContent, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import type { EffectsMode, EffectsSnapshot, SystemMotion } from '@/lib/ipc-bindings.generated'
import { initializeVisualEffects, INITIAL_EFFECTS } from '@/lib/visual-effects-store'
import '@/i18n'
import '@/styles/globals.css'

let mode: EffectsMode = (localStorage.getItem('smooth-fixture-mode') as EffectsMode) || 'auto'
let system: SystemMotion = 'allow'
let revision = 0
const listeners = new Set<(snapshot: EffectsSnapshot) => void>()
const channel = new BroadcastChannel('smooth-fixture')
const snapshot = (): EffectsSnapshot => ({
  ...INITIAL_EFFECTS,
  sessionId: 'browser-fixture',
  revision,
  mode,
  systemMotion: system,
  lowEffects: mode !== 'effects',
  reduceMotion: mode !== 'effects' || system !== 'allow',
  persistence: 'saved',
  reason: system !== 'allow' ? 'system' : mode === 'auto' ? 'platform_default' : 'manual',
})
visualEffectsApi.get = async () => snapshot()
visualEffectsApi.setMode = async next => {
  mode = next
  revision += 1
  localStorage.setItem('smooth-fixture-mode', mode)
  channel.postMessage({ mode, revision })
  const nextSnapshot = snapshot()
  listeners.forEach(listener => listener(nextSnapshot))
  return nextSnapshot
}
visualEffectsApi.environment = async (_, next) => {
  system = next
  revision += 1
  return snapshot()
}
visualEffectsApi.subscribe = async listener => {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
channel.onmessage = event => {
  mode = event.data.mode
  revision = Math.max(revision + 1, event.data.revision)
  listeners.forEach(listener => listener(snapshot()))
}
initializeVisualEffects()

export default function Fixture() {
  const [checked, setChecked] = useState(false)
  return (
    <LazyMotion features={domMax} strict>
      <VisualEffectsProvider>
        <main className="mx-auto max-w-xl p-6 text-foreground">
          <SmoothModeSetting />
          <label className="my-4 block">
            保留输入
            <input aria-label="保留输入" className="ml-2 border p-2" />
          </label>
          <Switch checked={checked} onCheckedChange={setChecked} aria-label="测试开关" />
          <m.div
            data-testid="motion-target"
            animate={{ opacity: checked ? 0.4 : 1, x: checked ? 80 : 0 }}
            transition={{ duration: 2 }}
            className="my-4 h-10 w-24 bg-primary"
          />
          <Dialog>
            <DialogTrigger className="border p-2">打开弹窗</DialogTrigger>
            <DialogContent>
              <DialogTitle>测试弹窗</DialogTitle>
              <input aria-label="弹窗输入" />
            </DialogContent>
          </Dialog>
        </main>
      </VisualEffectsProvider>
    </LazyMotion>
  )
}

createRoot(document.getElementById('root')!).render(<Fixture />)
