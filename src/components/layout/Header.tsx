import { m } from 'framer-motion'
import { ClipboardCopy, Star, FileText, Image, Link as LinkIcon, Folder, Code } from 'lucide-react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import { cn } from '@/lib/utils'

const FAVORITES_UI_ENABLED = false

interface HeaderProps {
  onFilterChange?: (filterId: Filter) => void
  className?: string
}

const Header: React.FC<HeaderProps> = ({ onFilterChange, className }) => {
  const { t } = useTranslation()

  const filterTypes = [
    { id: Filter.All, label: 'header.filters.all', icon: ClipboardCopy },
    // 后端收藏功能尚未实装：暂时隐藏"已收藏"筛选入口（保留逻辑，后续直接开关启用）。
    ...(FAVORITES_UI_ENABLED
      ? [{ id: Filter.Favorited, label: 'header.filters.favorited', icon: Star }]
      : []),
    { id: Filter.Text, label: 'header.filters.text', icon: FileText },
    { id: Filter.Image, label: 'header.filters.image', icon: Image },
    { id: Filter.Link, label: 'header.filters.link', icon: LinkIcon },
    { id: Filter.File, label: 'header.filters.file', icon: Folder },
    { id: Filter.Code, label: 'header.filters.code', icon: Code },
  ]

  const [activeFilter, setActiveFilter] = useState<Filter>(Filter.All)

  const handleFilterClick = (filterId: Filter) => {
    setActiveFilter(filterId)
    onFilterChange?.(filterId)
  }

  return (
    <header
      data-tauri-drag-region
      className={cn('shrink-0 px-6 transition-all duration-300', className)}
    >
      {/* Glass Background */}
      <div
        data-tauri-drag-region
        className="absolute inset-0 bg-background/60 backdrop-blur-xl border-b border-white/5 shadow-sm rounded-lg"
      />

      <div data-tauri-drag-region className="relative z-10">
        {/* Filter Buttons */}
        <div className="flex items-center gap-2 overflow-x-auto no-scrollbar py-2 -mx-6 px-6 mask-linear-fade">
          {filterTypes.map(filter => {
            const Icon = filter.icon
            const isActive = activeFilter === filter.id

            return (
              <m.button
                data-tauri-drag-region="false"
                key={filter.id}
                onClick={() => handleFilterClick(filter.id)}
                className={cn(
                  'relative group flex items-center gap-2 px-3.5 py-2 rounded-lg text-sm font-medium whitespace-nowrap transition-all duration-300 outline-none select-none',
                  isActive
                    ? 'text-primary-foreground'
                    : 'text-muted-foreground hover:text-foreground hover:bg-muted/50'
                )}
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.96 }}
              >
                {isActive && (
                  <m.div
                    layoutId="activeFilter"
                    className="absolute inset-0 bg-primary rounded-lg shadow-md shadow-primary/20"
                    transition={{ type: 'spring', bounce: 0.2, duration: 0.6 }}
                  />
                )}
                <span className="relative z-10 flex items-center gap-2">
                  <Icon className={cn('h-4 w-4', isActive ? 'text-primary-foreground' : '')} />
                  {t(filter.label)}
                </span>
              </m.button>
            )
          })}
        </div>
      </div>
    </header>
  )
}

export default Header
