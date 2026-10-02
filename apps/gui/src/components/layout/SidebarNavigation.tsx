import '@/components/ui/selection-item.css'
import { History, MonitorCog } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { NavLink, useLocation } from 'react-router'
import SelectionIndicator from '@/components/ui/selection-indicator'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

const routes = [
  { to: '/history', labelKey: 'nav.history', icon: History },
  { to: '/devices', labelKey: 'nav.devices', icon: MonitorCog },
] as const

function SidebarNavigation() {
  const { t } = useTranslation()
  const { pathname } = useLocation()

  return (
    <nav className="flex min-h-0 flex-1 flex-col items-center gap-1 px-2 py-2">
      <TooltipProvider delay={300}>
        {routes.map(route => {
          const Icon = route.icon
          const label = t(route.labelKey)

          return (
            <Tooltip key={route.to}>
              <TooltipTrigger
                render={
                  <NavLink
                    data-tauri-drag-region="false"
                    to={route.to}
                    aria-label={label}
                    className={({ isActive }) =>
                      cn(
                        'selection-item relative isolate flex size-8 items-center justify-center rounded-md text-muted-foreground',
                        isActive ? 'text-foreground' : 'hover:text-foreground'
                      )
                    }
                  />
                }
              >
                {pathname === route.to && (
                  <SelectionIndicator layoutId="sidebar-selection" className="bg-muted" />
                )}
                <Icon className="selection-item-content relative z-10 size-4" />
              </TooltipTrigger>
              <TooltipContent side="right" sideOffset={6}>
                {label}
              </TooltipContent>
            </Tooltip>
          )
        })}
      </TooltipProvider>
    </nav>
  )
}

export default SidebarNavigation
