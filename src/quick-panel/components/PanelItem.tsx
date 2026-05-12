import { FileText } from 'lucide-react'
import React from 'react'
import { isMac, typeIcons } from '../constants'
import type { DisplayItem } from '../types'
import { formatRelativeTime } from '@/lib/clipboard-utils'

interface PanelItemProps {
  item: DisplayItem
  index: number
  isSelected: boolean
  hoverDisabled: boolean
  onSelect: (index: number, plainOnly?: boolean) => void
  onHover: (index: number) => void
  itemRef?: React.Ref<HTMLDivElement>
  shortcutKey?: string
}

const PanelItem: React.FC<PanelItemProps> = React.memo(
  ({ item, index, isSelected, hoverDisabled, onSelect, onHover, itemRef, shortcutKey }) => {
    const Icon = typeIcons[item.type] ?? FileText
    const isUnavailable = item.isUnavailable

    return (
      <div
        ref={itemRef}
        className={[
          'flex cursor-pointer select-none items-center gap-2.5 rounded-md px-4 py-2 text-[13px] leading-tight transition-colors',
          isSelected
            ? 'bg-primary text-primary-foreground shadow-sm shadow-primary/20'
            : hoverDisabled
              ? 'text-foreground'
              : 'text-foreground hover:bg-muted/50',
        ].join(' ')}
        onClick={e => onSelect(index, e.altKey)}
        onMouseEnter={() => onHover(index)}
      >
        <Icon
          className={[
            'h-3.5 w-3.5 shrink-0',
            isSelected ? 'text-primary-foreground/70' : 'text-muted-foreground/60',
            isUnavailable && 'opacity-40',
          ]
            .filter(Boolean)
            .join(' ')}
        />
        <span
          className={['flex-1 truncate', isUnavailable && 'line-through opacity-60']
            .filter(Boolean)
            .join(' ')}
        >
          {item.preview || '(empty)'}
        </span>
        <span
          className={[
            'shrink-0 tabular-nums text-[11px]',
            isSelected ? 'text-primary-foreground/60' : 'text-muted-foreground/50',
          ].join(' ')}
        >
          {formatRelativeTime(item.activeTime)}
        </span>
        {shortcutKey && (
          <kbd
            className={[
              'shrink-0 rounded border px-1 py-0.5 font-mono text-[10px] leading-none',
              isSelected
                ? 'border-primary-foreground/30 text-primary-foreground/70'
                : 'border-border text-muted-foreground/50',
            ].join(' ')}
          >
            {isMac ? '⌘' : '⌃'}
            {shortcutKey}
          </kbd>
        )}
      </div>
    )
  }
)

export default PanelItem
