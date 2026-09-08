import { LazyMotion, domMax } from 'framer-motion'
import { type ReactNode, useCallback, useMemo, useState } from 'react'
import { BrowserRouter as Router, useNavigate } from 'react-router'
import { unlockEncryptionSession } from '@/api/security'
import { SidebarTitle, TitleBar } from '@/components'
import { AppContent } from '@/components/app/AppContent'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import { SearchProvider } from '@/contexts/SearchContext'
import { SettingProvider } from '@/contexts/SettingContext'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import { TitleBarSlotContext } from '@/contexts/titlebar-slot-context'
import { UpdateProvider } from '@/contexts/UpdateContext'
import { useUINavigateListener } from '@/hooks/useUINavigateListener'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { WindowShell } from '@/layouts'
import { isSetupGateActive } from '@/lib/app-state'
import { useSetupRealtimeStore } from '@/store/setupRealtimeStore'
import './App.css'

const handleSetupComplete = () => {
  unlockEncryptionSession().catch(error => console.warn('Post-setup auto-unlock failed:', error))
}

export default function App() {
  return (
    <LazyMotion features={domMax} strict>
      <VisualEffectsProvider>
        <Router>
          <SearchProvider>
            <SettingProvider>
              <UpdateProvider>
                <AppContentWithBar />
              </UpdateProvider>
            </SettingProvider>
          </SearchProvider>
        </Router>
      </VisualEffectsProvider>
    </LazyMotion>
  )
}

const TitleBarWithSearch = ({
  isSetupActive,
  rightSlot,
}: {
  isSetupActive: boolean
  rightSlot: ReactNode
}) => <TitleBar isSetupActive={isSetupActive} rightSlot={rightSlot} />

export const AppContentWithBar = () => {
  const { hasCustomTitleBar } = useWindowFrame()
  const { hydrated, flow } = useSetupRealtimeStore()
  const isSetupActive = isSetupGateActive(flow, hydrated)
  const navigate = useNavigate()
  const handleNavigate = useCallback((route: string) => navigate(route), [navigate])
  useUINavigateListener(handleNavigate)

  const [rightSlotHost, setRightSlotHost] = useState<HTMLDivElement | null>(null)
  const slotValue = useMemo(() => ({ rightSlotHost }), [rightSlotHost])
  const rightSlot = useMemo(() => <div ref={setRightSlotHost} />, [])
  const titleBar = useMemo(
    () =>
      hasCustomTitleBar ? (
        <TitleBarSlotContext value={slotValue}>
          <TitleBarWithSearch isSetupActive={isSetupActive} rightSlot={rightSlot} />
        </TitleBarSlotContext>
      ) : null,
    [hasCustomTitleBar, slotValue, isSetupActive, rightSlot]
  )
  const sidebarTitle = useMemo(
    () => (hasCustomTitleBar ? <SidebarTitle rightSlot={rightSlot} /> : null),
    [hasCustomTitleBar, rightSlot]
  )

  return (
    <TitleBarSlotContext value={slotValue}>
      <ShortcutProvider>
        <WindowShell titleBar={null}>
          <AppContent
            fullTitleBar={titleBar}
            isSetupActive={isSetupActive}
            onSetupComplete={handleSetupComplete}
            sidebarTitle={sidebarTitle}
          />
        </WindowShell>
      </ShortcutProvider>
    </TitleBarSlotContext>
  )
}
