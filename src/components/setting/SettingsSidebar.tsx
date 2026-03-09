import { ArrowLeft } from 'lucide-react'
import type { FC } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { SETTINGS_CATEGORIES } from '@/components/setting/settings-config'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuItem,
} from '@/components/ui/sidebar'
import { getSettingsIconPosition, startCircularCollapse } from '@/lib/theme-transition'

interface SettingsSidebarProps {
  activeCategory: string
  onCategoryChange: (category: string) => void
}

const SettingsSidebar: FC<SettingsSidebarProps> = ({ activeCategory, onCategoryChange }) => {
  const { t } = useTranslation()
  const navigate = useNavigate()

  const handleBack = () => {
    const doNavigate = () => {
      if (window.history.state && window.history.state.idx > 0) {
        navigate(-1)
      } else {
        navigate('/')
      }
    }
    const { x, y } = getSettingsIconPosition()
    startCircularCollapse(x, y, doNavigate)
  }

  return (
    <Sidebar collapsible="none" className="border-r border-border/50 bg-muted/30">
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              {SETTINGS_CATEGORIES.map(item => {
                const Icon = item.icon
                const isActive = activeCategory === item.id

                return (
                  <SidebarMenuItem key={item.id}>
                    <button
                      type="button"
                      onClick={() => onCategoryChange(item.id)}
                      className={`flex w-full items-center gap-2 overflow-hidden rounded-md p-2 text-left text-sm outline-none ring-sidebar-ring transition-[width,height,padding] focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-50 [&>span:last-child]:truncate [&>svg]:size-4 [&>svg]:shrink-0 ${
                        isActive
                          ? 'bg-primary/10 font-medium text-primary'
                          : 'text-muted-foreground hover:bg-muted hover:text-foreground'
                      }`}
                    >
                      <Icon className="h-4 w-4" />
                      <span>{t(`settings.categories.${item.id}`)}</span>
                    </button>
                  </SidebarMenuItem>
                )
              })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <button
                  type="button"
                  onClick={handleBack}
                  className="flex w-full items-center gap-2 overflow-hidden rounded-md p-2 text-left text-sm outline-none ring-sidebar-ring transition-[width,height,padding] focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-50 [&>span:last-child]:truncate [&>svg]:size-4 [&>svg]:shrink-0 text-muted-foreground hover:bg-muted hover:text-foreground"
                >
                  <ArrowLeft className="h-4 w-4" />
                  <span>{t('nav.back')}</span>
                </button>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarFooter>
    </Sidebar>
  )
}

export default SettingsSidebar
