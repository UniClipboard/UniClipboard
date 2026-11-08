import { LazyMotion, domMax } from 'framer-motion'
import { useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { ReleaseNotificationCenter } from '@/components/notifications/ReleaseNotificationCenter'
import { ShortcutContext, type ShortcutContextType } from '@/contexts/shortcut-context'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { HistoryManagementRail } from '@/pages/history-kit/HistoryManagementRail'
import { useHistoryManagement } from '@/pages/history-kit/useHistoryManagement'
import { store } from '@/store'
import { Pane, SidebarNav, SplitView, UiRoot } from '@/ui-kit'
import { installSettingsFixtureEnvironment } from './settings-environment'
import '@/i18n'
import '@/styles/globals.css'

installSettingsFixtureEnvironment()

export default function Fixture() {
  const scheme = useAppliedColorScheme()
  const [hidden, setHidden] = useState(false)
  const notifications = useHistoryManagement(
    () => {},
    () => {}
  )
  const shortcuts = useMemo<ShortcutContextType>(
    () => ({
      activeScope: 'history',
      activeLayer: 'page',
      activePriority: 0,
      pushLayer: () => 'fixture',
      popLayer: () => {},
    }),
    []
  )
  return (
    <ShortcutContext value={shortcuts}>
      <LazyMotion features={domMax} strict>
        <div className="h-screen min-h-0">
          <UiRoot colorScheme={scheme}>
            <SplitView
              sidebarHidden={hidden}
              title="剪贴板历史"
              sidebarToggle={{
                label: hidden ? '显示侧边栏' : '隐藏侧边栏',
                onClick: () => setHidden(value => !value),
              }}
              sidebar={
                <SidebarNav
                  label="资料库"
                  sections={[
                    {
                      id: 'library',
                      items: [{ id: 'history', label: '剪贴板历史', icon: 'history' }],
                    },
                  ]}
                  footer={notifications.management.items}
                  onValueChange={notifications.management.onSelect}
                />
              }
              collapsedSidebar={<HistoryManagementRail management={notifications.management} />}
            >
              <Pane kind="main" label="剪贴板历史">
                <p className="p-4 text-ui-body text-muted-foreground">合成剪贴板历史</p>
              </Pane>
            </SplitView>
            <ReleaseNotificationCenter
              open={notifications.open}
              onOpenChange={notifications.setOpen}
            />
          </UiRoot>
        </div>
      </LazyMotion>
    </ShortcutContext>
  )
}
createRoot(document.getElementById('root')!).render(
  <Provider store={store}>
    <Fixture />
  </Provider>
)
