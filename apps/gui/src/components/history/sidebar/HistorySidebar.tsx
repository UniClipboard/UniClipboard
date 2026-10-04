import { Inbox, MonitorCog, Pin, Settings, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { NavLink } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import { ThemeToggle } from '@/components/motion/theme-toggle'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useAppSelector } from '@/store/hooks'
import type { HistorySidebarContext } from './history-sidebar-types'
import HistorySidebarNavItem from './HistorySidebarNavItem'
import HistorySidebarSection from './HistorySidebarSection'

interface HistorySidebarProps {
  context: HistorySidebarContext
  activeFilter: Filter
  onSelectAllItems: () => void
  onSelectPinned: () => void
}

/** Shared Library-style sidebar for the History and Devices top-level pages
 * (history-window-exec-plan.md slice 1 / HSidebar.dc.html). Smart Views and
 * Tags render as empty shells this round - see "已决定" in the exec plan. */
function HistorySidebar({
  context,
  activeFilter,
  onSelectAllItems,
  onSelectPinned,
}: HistorySidebarProps) {
  const { t } = useTranslation()
  const [smartViewsOpen, setSmartViewsOpen] = useState(false)
  const [tagsOpen, setTagsOpen] = useState(false)
  const [devicesOpen, setDevicesOpen] = useState(context === 'devices')

  const spaceMembers = useAppSelector(state => state.devices.spaceMembers)
  const onlineCount = spaceMembers.filter(member => member.connected).length

  return (
    <aside className="flex w-56 shrink-0 flex-col border-r border-border/50 bg-muted/15 xl:w-60">
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col px-2 pb-3 pt-2">
          <HistorySidebarNavItem
            icon={Inbox}
            label={t('history.sidebar.allItems')}
            active={context === 'history' && activeFilter === Filter.All}
            onClick={onSelectAllItems}
          />
          <HistorySidebarNavItem
            icon={Pin}
            label={t('history.sidebar.pinned')}
            active={context === 'history' && activeFilter === Filter.Favorited}
            onClick={onSelectPinned}
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
              <span className="shrink-0 text-ui-caption text-muted-foreground/70">
                {t('history.sidebar.devicesOnline', {
                  online: onlineCount,
                  total: spaceMembers.length,
                })}
              </span>
            }
          >
            {spaceMembers.length === 0 ? (
              <p className="px-2.5 py-1.5 text-ui-caption text-muted-foreground/70">
                {t('history.sidebar.devicesEmpty')}
              </p>
            ) : (
              spaceMembers.map(member => (
                <HistorySidebarNavItem
                  key={member.peerId}
                  icon={MonitorCog}
                  label={member.deviceName}
                  trailing={
                    <span
                      aria-hidden="true"
                      className={
                        member.connected
                          ? 'size-1.5 shrink-0 rounded-full bg-success'
                          : 'size-1.5 shrink-0 rounded-full bg-muted-foreground/40'
                      }
                    />
                  }
                />
              ))
            )}
          </HistorySidebarSection>
        </div>
      </ScrollArea>

      <div className="flex items-center justify-between gap-2 border-t border-border/50 px-3 py-2">
        <NavLink
          to="/settings"
          className="flex min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-ui-body text-muted-foreground hover:text-foreground"
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
