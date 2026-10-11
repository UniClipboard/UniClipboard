import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { HistoryTagColorDto, HistoryTagDto } from '@/api/daemon/history-tags'
import {
  MAX_TAG_NAME_LENGTH,
  tagSuggestions,
  type TagSuggestion,
} from '@/components/history/detail/history-tag-suggestions'
import { DEFAULT_TAG_COLOR, TAG_COLORS } from '@/lib/tag-colors'
import { ComboField, type SearchFieldKey } from '@/ui-kit'

/**
 * The tag field of an item or of the selection (design MC1): type to filter this device's tags or
 * name a new one; ↑↓ choose, Return adds, Tab cycles a new tag's colour, Escape closes. The new
 * tag's colours are the product's tag palette; the design's eight system colours are not offered.
 */
export function HistoryKitTagEditor({
  tags,
  attachedIds,
  onPick,
  onClose,
}: {
  tags: HistoryTagDto[]
  attachedIds: ReadonlySet<string>
  /** Attach an existing tag or create-and-attach one in `color`; resolves to success. */
  onPick: (suggestion: TagSuggestion, color: HistoryTagColorDto) => Promise<boolean>
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const [highlight, setHighlight] = useState(0)
  const [color, setColor] = useState<HistoryTagColorDto>(DEFAULT_TAG_COLOR)
  const [busy, setBusy] = useState(false)
  const suggestions = tagSuggestions(query, tags, attachedIds)
  const active = Math.min(highlight, suggestions.length - 1)
  const firstExisting = suggestions.findIndex(s => s.kind === 'existing')

  const pick = async (suggestion: TagSuggestion | undefined) => {
    if (!suggestion || busy) return
    setBusy(true)
    const ok = await onPick(suggestion, color)
    setBusy(false)
    if (ok) onClose()
  }
  const keyDown = (key: SearchFieldKey) => {
    if (key.key === 'ArrowDown' || key.key === 'ArrowUp') {
      key.preventDefault()
      if (suggestions.length === 0) return
      const step = key.key === 'ArrowDown' ? 1 : -1
      setHighlight((active + step + suggestions.length) % suggestions.length)
    } else if (key.key === 'Enter') {
      key.preventDefault()
      void pick(suggestions[active])
    } else if (key.key === 'Tab' && suggestions[active]?.kind === 'create') {
      key.preventDefault()
      const at = TAG_COLORS.indexOf(color as (typeof TAG_COLORS)[number])
      const step = key.shiftKey ? -1 : 1
      setColor(TAG_COLORS[(at + step + TAG_COLORS.length) % TAG_COLORS.length])
    } else if (key.key === 'Escape') {
      key.preventDefault()
      onClose()
    }
  }
  return (
    <ComboField
      label={t('history.tags.inputLabel')}
      prefix="#"
      maxLength={MAX_TAG_NAME_LENGTH}
      value={query}
      onValueChange={next => {
        setQuery(next)
        setHighlight(0)
      }}
      suggestions={suggestions.map((suggestion, index) =>
        suggestion.kind === 'create'
          ? {
              id: 'create',
              // The skin draws the plus; the shared string carries its own.
              label: t('history.tags.createNamed', { name: suggestion.name }).replace(/^\+\s*/, ''),
              create: true,
            }
          : {
              id: suggestion.tag.tagId,
              label: `#${suggestion.tag.name ?? ''}`,
              detail: t('history.subtitle', { count: suggestion.tag.entryCount }),
              group:
                index === firstExisting
                  ? t(
                      suggestion.group === 'similar'
                        ? 'history.tags.similar'
                        : 'history.tags.frequent'
                    )
                  : undefined,
            }
      )}
      highlighted={suggestions.length > 0 ? active : -1}
      onHighlightedChange={setHighlight}
      onSelectSuggestion={index => void pick(suggestions[index])}
      onKeyDown={keyDown}
      onDismiss={onClose}
      createColor={{
        label: t('history.tags.color'),
        colors: [...TAG_COLORS],
        value: TAG_COLORS.includes(color as (typeof TAG_COLORS)[number])
          ? (color as (typeof TAG_COLORS)[number])
          : DEFAULT_TAG_COLOR,
        onValueChange: next => setColor(next),
      }}
      footer={t('history.kit.tagFieldHint')}
    />
  )
}
