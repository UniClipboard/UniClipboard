import '@/components/ui/selection-item.css'
import { LayoutGroup } from 'framer-motion'
import { ArrowLeft } from 'lucide-react'
import { useId, type FC } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { SETTINGS_CATEGORIES } from '@/components/setting/settings-config'
import SelectionIndicator from '@/components/ui/selection-indicator'
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
  const selectionId = useId()
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
        <LayoutGroup id={selectionId}>
          <SidebarGroup>
            <SidebarGroupContent>
              <SidebarMenu className="gap-1">
                {SETTINGS_CATEGORIES.map(item => {
                  const Icon = item.icon
                  const isActive = activeCategory === item.id

                  return (
                    <SidebarMenuItem key={item.id}>
                      <button
                        type="button"
                        onClick={() => onCategoryChange(item.id)}
                        aria-current={isActive ? 'true' : undefined}
                        className={`selection-item relative isolate flex w-full items-center gap-2 rounded-lg px-3 py-2.5 text-left text-sm outline-none ring-sidebar-ring focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-50 [&>span:last-child]:truncate [&>svg]:size-4 [&>svg]:shrink-0 ${
                          isActive
                            ? 'font-medium text-foreground'
                            : 'text-muted-foreground hover:text-foreground'
                        }`}
                      >
                        {isActive && (
                          <SelectionIndicator
                            layoutId="settings-selection"
                            className="bg-foreground/[0.06] dark:bg-foreground/10"
                          />
                        )}
                        <Icon className="selection-item-content relative z-10 size-4" />
                        <span className="selection-item-content relative z-10">
                          {t(`settings.categories.${item.id}`)}
                        </span>
                      </button>
                    </SidebarMenuItem>
                  )
                })}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </LayoutGroup>
      </SidebarContent>
      <SidebarFooter>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <button
                  type="button"
                  onClick={handleBack}
                  className="selection-item relative isolate flex w-full items-center gap-2 overflow-hidden rounded-md p-2 text-left text-sm outline-none ring-sidebar-ring focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-50 [&>span:last-child]:truncate [&>svg]:size-4 [&>svg]:shrink-0 text-muted-foreground hover:text-foreground"
                >
                  <ArrowLeft className="selection-item-content relative z-10 size-4" />
                  <span className="selection-item-content relative z-10">{t('nav.back')}</span>
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
