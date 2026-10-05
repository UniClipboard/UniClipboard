import { useTranslation } from 'react-i18next'
import type { HistoryTagColorDto } from '@/api/daemon/history-tags'
import { ContextMenuItem } from '@/components/motion/context-menu'
import { isCustomColor, TAG_COLORS, tagTint } from '@/lib/tag-colors'
import { cn } from '@/lib/utils'

interface HistoryTagColorMenuProps {
  /** The tag's color; absent for a local tag that has none (gray). */
  color: HistoryTagColorDto | undefined
  onChange: (color: HistoryTagColorDto) => void
  /** Open the full color picker for any other color. */
  onCustom: () => void
}

const SWATCH_ITEM = 'w-auto justify-center rounded-full p-1'
const CHECKED = 'ring-2 ring-foreground ring-offset-2 ring-offset-popover'

/**
 * The "Color" block of a tag's actions (HManage.dc.html row menu): the palette
 * as a row of swatches, then one that opens any color. Each swatch is a menu
 * item, so the arrow keys reach it like any other action.
 */
function HistoryTagColorMenu({ color, onChange, onCustom }: HistoryTagColorMenuProps) {
  const { t } = useTranslation()
  const custom = color !== undefined && isCustomColor(color)
  const customTint = tagTint(color)
  return (
    <div className="flex flex-col gap-1 px-2.5 pb-2 pt-1.5">
      <span className="text-ui-body">{t('history.tags.color')}</span>
      <div role="group" aria-label={t('history.tags.color')} className="-mx-1 flex items-center">
        {TAG_COLORS.map(swatch => (
          <ContextMenuItem
            key={swatch}
            textValue={t(`history.tags.colors.${swatch}`)}
            onSelect={() => onChange(swatch)}
            className={SWATCH_ITEM}
          >
            <span
              aria-hidden="true"
              className={cn(
                'size-4.5 rounded-full',
                tagTint(swatch).dot,
                swatch === color && CHECKED
              )}
            />
            <span className="sr-only">
              {t(`history.tags.colors.${swatch}`)}
              {swatch === color ? ` (${t('history.tags.currentColor')})` : ''}
            </span>
          </ContextMenuItem>
        ))}
        <ContextMenuItem
          textValue={t('history.tags.customColorEllipsis')}
          onSelect={onCustom}
          className={SWATCH_ITEM}
        >
          <span
            aria-hidden="true"
            style={custom ? customTint.style : undefined}
            className={cn(
              'size-4.5 rounded-full',
              custom
                ? cn(customTint.dot, CHECKED)
                : 'bg-[conic-gradient(#d8743a,#d9b23a,#2f9e6a,#3e9fa8,#3e5fa8,#7a3e8f,#c0467a,#d8743a)]'
            )}
          />
          <span className="sr-only">{t('history.tags.customColorEllipsis')}</span>
        </ContextMenuItem>
      </div>
    </div>
  )
}

export default HistoryTagColorMenu
