import { useState } from 'react'
import { HexColorPicker } from 'react-colorful'
import { useTranslation } from 'react-i18next'
import type { HistoryTagColorDto } from '@/api/daemon/history-tags'
import { isCustomColor, TAG_COLORS, tagTint } from '@/lib/tag-colors'
import { cn } from '@/lib/utils'

interface TagColorPickerProps {
  /** The current color: a palette name, a `#rrggbb`, or none. */
  value: HistoryTagColorDto | undefined
  onChange: (color: HistoryTagColorDto) => void
  /** Show the hex field. Off inside the tag editor, whose name field must
   * keep the keyboard focus. */
  hexInput?: boolean
}

/** A fallback for the color area while the tag has a palette color or none. */
const AREA_START = '#3e5fa8'

/**
 * A tag's color beyond the palette: the five palette swatches, then a
 * saturation/hue area for any color (react-colorful, as the appearance
 * settings use) and its `#rrggbb` field. Reports every change; the caller
 * decides when to save.
 */
function TagColorPicker({ value, onChange, hexInput = true }: TagColorPickerProps) {
  const { t } = useTranslation()
  const custom = value && isCustomColor(value) ? value.toLowerCase() : null
  // What the hex field shows while it is being typed in.
  const [draft, setDraft] = useState<string | null>(null)

  const typeHex = (raw: string) => {
    const hex = raw.trim().startsWith('#') ? raw.trim() : `#${raw.trim()}`
    setDraft(raw)
    if (isCustomColor(hex)) onChange(hex.toLowerCase())
  }

  return (
    <div className="flex w-52 flex-col gap-3">
      <div role="radiogroup" aria-label={t('history.tags.color')} className="flex gap-2">
        {TAG_COLORS.map(swatch => (
          <button
            key={swatch}
            type="button"
            role="radio"
            aria-checked={swatch === value}
            aria-label={t(`history.tags.colors.${swatch}`)}
            onClick={() => onChange(swatch)}
            className={cn(
              'size-6 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring/50',
              tagTint(swatch).dot,
              swatch === value && 'ring-2 ring-foreground ring-offset-2 ring-offset-popover'
            )}
          />
        ))}
      </div>
      <HexColorPicker
        aria-label={t('history.tags.customColor')}
        color={custom ?? AREA_START}
        onChange={color => {
          setDraft(null)
          onChange(color.toLowerCase())
        }}
        style={{ width: '100%', height: '8.5rem' }}
      />
      {hexInput && (
        <input
          aria-label={t('history.tags.hexColor')}
          value={draft ?? custom ?? ''}
          placeholder="#rrggbb"
          onChange={event => typeHex(event.target.value)}
          onBlur={() => setDraft(null)}
          spellCheck={false}
          maxLength={7}
          className="h-8 w-full rounded-md border border-border bg-background px-2 font-mono text-ui-body outline-none focus:border-foreground"
        />
      )}
    </div>
  )
}

export default TagColorPicker
