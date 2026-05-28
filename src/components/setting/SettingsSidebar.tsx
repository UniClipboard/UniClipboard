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

interface SettingsSidebarProps {
  activeCategory: string
  onCategoryChange: (category: string) => void
  /**
   * Linux/Tauri 下设置页改用扁平布局，侧栏需要显式边框来代替原本由 InsetSurface 提供的视觉分隔。
   */
  flat?: boolean
}

const SettingsSidebar: FC<SettingsSidebarProps> = ({
  activeCategory,
  onCategoryChange,
  flat = false,
}) => {
  const { t } = useTranslation()
  const navigate = useNavigate()

  const handleBack = () => {
    if (window.history.state && window.history.state.idx > 0) {
      navigate(-1)
    } else {
      navigate('/')
    }
  }

  return (
    <Sidebar
      collapsible="none"
      className={
        flat
          ? 'border-r border-border/40 bg-background/80 dark:bg-background/60'
          : 'bg-transparent border-none'
      }
    >
      <SidebarContent className={flat ? '' : 'bg-transparent'}>
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
                      <Icon className="size-4" />
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
                  <ArrowLeft className="size-4" />
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
