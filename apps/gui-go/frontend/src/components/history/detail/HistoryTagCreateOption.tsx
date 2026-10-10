import { useTranslation } from 'react-i18next'
import type { HistoryTagColorDto } from '@/api/daemon/history-tags'
import TagColorPicker from '@/components/history/tags/TagColorPicker'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { isCustomColor, TAG_COLORS, tagTint } from '@/lib/tag-colors'
import { cn } from '@/lib/utils'

interface HistoryTagCreateOptionProps {
  /** The option's DOM id, for the field's `aria-activedescendant`. */
  id: string
  name: string
  color: HistoryTagColorDto
  onColorChange: (color: HistoryTagColorDto) => void
  active: boolean
  onPick: () => void
  onHighlight: () => void
  /** The custom color popover; the editor keeps itself open while it shows. */
  pickerOpen: boolean
  onPickerOpenChange: (open: boolean) => void
}

/**
 * The tag editor's "Create #name" option (HDetail.dc.html `tagging`): the new
 * tag as it will look, then the five palette colors (⇥ cycles them while the
 * option is highlighted) and a last swatch that opens any color.
 */
function HistoryTagCreateOption({
  id,
  name,
  color,
  onColorChange,
  active,
  onPick,
  onHighlight,
  pickerOpen,
  onPickerOpenChange,
}: HistoryTagCreateOptionProps) {
  const { t } = useTranslation()
  const custom = isCustomColor(color)
  const tint = tagTint(color)
  return (
    <div
      id={id}
      role="option"
      aria-selected={active}
      onMouseEnter={onHighlight}
      className={cn('flex flex-col gap-2.5 rounded-lg p-2.5', active && 'bg-history-accent-soft')}
    >
      <button
        type="button"
        tabIndex={-1}
        onClick={onPick}
        className="flex w-full items-center gap-2 text-left text-ui-body"
      >
        <span className="font-semibold">{t('history.tags.create')}</span>
        <span
          style={tint.style}
          className={cn(
            'inline-flex h-5.5 min-w-0 items-center truncate rounded-full px-2 text-ui-caption font-semibold',
            tint.chip
          )}
        >
          #{name}
        </span>
        <span className="ml-auto font-mono text-ui-caption text-muted-foreground">↵</span>
      </button>
      <div
        role="radiogroup"
        aria-label={t('history.tags.color')}
        className="flex items-center gap-1.5"
      >
        <span className="mr-1 text-ui-caption text-muted-foreground">
          {t('history.tags.color')}
        </span>
        {TAG_COLORS.map(swatch => (
          <button
            key={swatch}
            type="button"
            role="radio"
            tabIndex={-1}
            aria-checked={swatch === color}
            aria-label={t(`history.tags.colors.${swatch}`)}
            onClick={() => onColorChange(swatch)}
            className={cn(
              'size-5 rounded-full',
              tagTint(swatch).dot,
              swatch === color && 'ring-2 ring-foreground ring-offset-2 ring-offset-popover'
            )}
          />
        ))}
        <Popover open={pickerOpen} onOpenChange={onPickerOpenChange}>
          <PopoverTrigger
            render={
              <button
                type="button"
                role="radio"
                tabIndex={-1}
                aria-checked={custom}
                aria-label={t('history.tags.customColor')}
                style={custom ? tint.style : undefined}
                className={cn(
                  'size-5 rounded-full',
                  custom
                    ? cn(tint.dot, 'ring-2 ring-foreground ring-offset-2 ring-offset-popover')
                    : 'bg-[conic-gradient(#d8743a,#d9b23a,#2f9e6a,#3e9fa8,#3e5fa8,#7a3e8f,#c0467a,#d8743a)]'
                )}
              />
            }
          />
          <PopoverContent align="start" className="w-auto p-3">
            <TagColorPicker value={color} onChange={onColorChange} hexInput={false} />
          </PopoverContent>
        </Popover>
        <span className="ml-auto font-mono text-ui-caption text-muted-foreground">⇥</span>
      </div>
    </div>
  )
}

export default HistoryTagCreateOption
