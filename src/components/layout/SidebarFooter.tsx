import '@/components/ui/selection-item.css'
import { m } from 'framer-motion'
import { Settings } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { NavLink, useLocation } from 'react-router'
import DevProfileIndicator from '@/components/DevProfileIndicator'
import SelectionIndicator from '@/components/ui/selection-indicator'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

function SidebarFooter() {
  const { t } = useTranslation()
  const { pathname } = useLocation()
  const settingsLabel = t('nav.settings')

  return (
    <m.footer
      data-tauri-drag-region
      layout
      className="relative flex shrink-0 flex-col items-center gap-1 border-t border-border/40 px-2 py-2"
      transition={{ type: 'spring', stiffness: 420, damping: 34 }}
    >
      <div
        data-tauri-drag-region
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 flex items-center justify-center opacity-35"
      >
        <DevProfileIndicator compact />
      </div>
      <TooltipProvider delay={300}>
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
                      'selection-item relative isolate flex size-8 items-center justify-center rounded-md text-muted-foreground',
                      isActive ? 'text-foreground' : 'hover:text-foreground'
                    )
                  }
                />
              }
            >
              {pathname.startsWith('/settings') && (
                <SelectionIndicator layoutId="sidebar-selection" className="bg-muted" />
              )}
              <Settings className="selection-item-content relative z-10 size-4" />
            </TooltipTrigger>
            <TooltipContent side="right" sideOffset={6}>
              {settingsLabel}
            </TooltipContent>
          </Tooltip>
        </m.div>
      </TooltipProvider>
    </m.footer>
  )
}

export default SidebarFooter
