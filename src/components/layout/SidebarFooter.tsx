import { m } from 'framer-motion'
import { MonitorCog, Settings } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { NavLink } from 'react-router'
import DevProfileIndicator from '@/components/DevProfileIndicator'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

function SidebarFooter({
  collapsed,
  onOpenDevices,
}: {
  collapsed: boolean
  onOpenDevices: () => void
}) {
  const { t } = useTranslation()
  const devicesLabel = t('nav.devices')
  const settingsLabel = t('nav.settings')

  return (
    <m.footer
      data-tauri-drag-region
      layout
      className={cn(
        'relative flex shrink-0 items-center gap-1 overflow-hidden border-t border-border/40',
        collapsed ? 'flex-col px-2 py-2' : 'h-12 px-3'
      )}
      transition={{ type: 'spring', stiffness: 420, damping: 34 }}
    >
      <div
        data-tauri-drag-region
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 flex items-center justify-center opacity-35"
      >
        <DevProfileIndicator compact={collapsed} />
      </div>
      <TooltipProvider delay={300}>
        <m.div layout transition={{ type: 'spring', stiffness: 420, damping: 34 }}>
          <Tooltip>
            <TooltipTrigger
              render={
                <button
                  data-tauri-drag-region="false"
                  type="button"
                  aria-label={devicesLabel}
                  onClick={onOpenDevices}
                  className="flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                />
              }
            >
              <MonitorCog className="size-4" />
            </TooltipTrigger>
            <TooltipContent side={collapsed ? 'right' : 'top'} sideOffset={6}>
              {devicesLabel}
            </TooltipContent>
          </Tooltip>
        </m.div>

        <m.div layout transition={{ type: 'spring', stiffness: 420, damping: 34 }}>
          <Tooltip>
            <TooltipTrigger
              render={
                <NavLink
                  data-tauri-drag-region="false"
                  to="/settings"
                  aria-label={settingsLabel}
                  className={({ isActive }) =>
                    cn(
                      'flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground',
                      isActive && 'bg-muted text-foreground'
                    )
                  }
                />
              }
            >
              <Settings className="size-4" />
            </TooltipTrigger>
            <TooltipContent side={collapsed ? 'right' : 'top'} sideOffset={6}>
              {settingsLabel}
            </TooltipContent>
          </Tooltip>
        </m.div>
      </TooltipProvider>
    </m.footer>
  )
}

export default SidebarFooter
