import { Inbox, MonitorCog, Pin, Settings, Smartphone, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { NavLink, useNavigate } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import { ThemeToggle } from '@/components/motion/theme-toggle'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import { usePlatform } from '@/hooks/usePlatform'
import { useWindowDragging } from '@/hooks/useWindowDragging'
import { isMobileDeviceActive } from '@/lib/mobile-device-status'
import { cn } from '@/lib/utils'
import { useAppSelector } from '@/store/hooks'
import { HISTORY_LIBRARY_FILTER_STATE, type HistorySidebarProps } from './history-sidebar-types'
import HistorySidebarNavItem from './HistorySidebarNavItem'
import HistorySidebarSection from './HistorySidebarSection'

/** Shared Library sidebar of the History and Devices top-level pages
 * (HSidebar.dc.html). On macOS it is the window's left edge: it owns the
 * traffic-light strip and replaces the icon rail as top-level navigation.
 * Smart Views and Tags are empty shells this round (exec plan, "已决定"). */
function HistorySidebar(props: HistorySidebarProps) {
  const { context } = props
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { isMac } = usePlatform()
  const windowDragging = useWindowDragging()
  const [smartViewsOpen, setSmartViewsOpen] = useState(false)
  const [tagsOpen, setTagsOpen] = useState(false)
  const [devicesOpen, setDevicesOpen] = useState(context === 'devices')

  const spaceMembers = useAppSelector(state => state.devices.spaceMembers)
  const mobileDevices = useMobileDeviceList()
  const now = Date.now()
  const devices = [
    ...spaceMembers.map(m => ({
      id: m.peerId,
      name: m.deviceName,
      online: m.connected,
      mobile: false,
    })),
    ...mobileDevices.map(d => ({
      id: d.deviceId,
      name: d.label,
      online: isMobileDeviceActive(d, now),
      mobile: true,
    })),
  ]
  const onlineCount = devices.filter(d => d.online).length

  const selectLibrary = (filter: Filter.All | Filter.Favorited) => {
    if (props.context === 'history') props.onSelectLibrary(filter)
    else navigate('/history', { state: { [HISTORY_LIBRARY_FILTER_STATE]: filter } })
  }
  const libraryActive = (filter: Filter) =>
    props.context === 'history' && props.activeFilter === filter

  return (
    <aside className="flex w-55 shrink-0 flex-col border-r border-border/50 bg-sidebar text-sidebar-foreground">
      {isMac && (
        <div
          data-tauri-drag-region
          {...windowDragging}
          className="h-11 shrink-0"
          aria-hidden="true"
        />
      )}
      <ScrollArea className="min-h-0 flex-1">
        <nav
          aria-label={t('history.sidebar.library')}
          className={cn('flex flex-col px-2.5 pb-3', !isMac && 'pt-2')}
        >
          <HistorySidebarNavItem
            icon={Inbox}
            label={t('history.sidebar.allItems')}
            active={libraryActive(Filter.All)}
            onClick={() => selectLibrary(Filter.All)}
          />
          <HistorySidebarNavItem
            icon={Pin}
            label={t('history.sidebar.pinned')}
            active={libraryActive(Filter.Favorited)}
            onClick={() => selectLibrary(Filter.Favorited)}
          />
          <HistorySidebarNavItem
            icon={Trash2}
            label={t('history.sidebar.recentlyDeleted')}
            disabled
          />

          <HistorySidebarSection
            label={t('history.sidebar.smartViews')}
            open={smartViewsOpen}
            onOpenChange={setSmartViewsOpen}
          >
            <p className="px-2.5 py-1.5 text-ui-caption text-muted-foreground/70">
              {t('history.sidebar.smartViewsEmpty')}
            </p>
          </HistorySidebarSection>

          <HistorySidebarSection
            label={t('history.sidebar.tags')}
            open={tagsOpen}
            onOpenChange={setTagsOpen}
          >
            <p className="px-2.5 py-1.5 text-ui-caption text-muted-foreground/70">
              {t('history.sidebar.tagsEmpty')}
            </p>
          </HistorySidebarSection>

          <HistorySidebarSection
            label={t('history.sidebar.devices')}
            open={devicesOpen}
            onOpenChange={setDevicesOpen}
            trailing={
              context === 'history' ? (
                <NavLink
                  to="/devices"
                  className="shrink-0 text-ui-caption font-medium text-primary hover:underline"
                >
                  {t('history.sidebar.manageDevices')}
                </NavLink>
              ) : (
                <span className="shrink-0 text-ui-caption text-muted-foreground/70">
                  {t('history.sidebar.devicesOnline', {
                    online: onlineCount,
                    total: devices.length,
                  })}
                </span>
              )
            }
          >
            {devices.length === 0 ? (
              <p className="px-2.5 py-1.5 text-ui-caption text-muted-foreground/70">
                {t('history.sidebar.devicesEmpty')}
              </p>
            ) : (
              devices.map(device => (
                <HistorySidebarNavItem
                  key={device.id}
                  icon={device.mobile ? Smartphone : MonitorCog}
                  label={device.name}
                  trailing={
                    <span
                      aria-hidden="true"
                      className={cn(
                        'size-1.5 shrink-0 rounded-full',
                        device.online ? 'bg-success' : 'bg-muted-foreground/40'
                      )}
                    />
                  }
                />
              ))
            )}
          </HistorySidebarSection>
        </nav>
      </ScrollArea>

      <div className="flex items-center justify-between gap-2 px-2.5 py-2">
        <NavLink
          to="/settings"
          className="flex min-w-0 items-center gap-2 rounded-md px-2.5 py-1 text-ui-body text-muted-foreground hover:bg-muted/60 hover:text-foreground"
        >
          <Settings className="size-3.5 shrink-0" aria-hidden="true" />
          <span className="truncate">{t('history.sidebar.settings')}</span>
        </NavLink>
        <ThemeToggle className="size-7 shrink-0 rounded-md text-muted-foreground hover:bg-muted/60 hover:text-foreground" />
      </div>
    </aside>
  )
}

export default HistorySidebar
