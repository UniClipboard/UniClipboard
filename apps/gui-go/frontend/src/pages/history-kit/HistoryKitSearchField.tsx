import { useTranslation } from 'react-i18next'
import {
  DIMENSION_LABEL_KEYS,
  type Dimension,
} from '@/components/history/composite-search/composite-search-model'
import type { useCompositeSearchBar } from '@/components/history/composite-search/useCompositeSearchBar'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { TokenSearchField, type SearchFieldToken } from '@/ui-kit'

type SearchModel = ReturnType<typeof useCompositeSearchBar>

const KIND: Record<Dimension, SearchFieldToken['kind']> = {
  tag: 'tag',
  source: 'source',
  time: 'time',
  type: 'type',
  extension: 'other',
}

/**
 * The window search field (design MB1/MB2): the composite search model's filters are tokens in
 * the field, its candidates (with hit counts under the other filters) are the suggestions, and it
 * decides what every key does.
 */
export function HistoryKitSearchField({
  search,
  inputRef,
}: {
  search: SearchModel
  inputRef: React.RefObject<HTMLInputElement | null>
}) {
  const { t } = useTranslation()
  return (
    <TokenSearchField
      label={t('history.kit.searchLabel')}
      placeholder={t('history.listSearchPlaceholder')}
      shortcutHint={formatShortcutHint('mod+f')}
      value={search.buffer}
      onValueChange={search.changeText}
      tokens={search.chips.map(chip => ({
        id: chip.dimension,
        field: t(DIMENSION_LABEL_KEYS[chip.dimension]),
        value: chip.label,
        kind: KIND[chip.dimension],
        remove: {
          label: t('history.composite.removeFilter', { filter: chip.label }),
          onClick: () => search.resetDimension(chip.dimension),
        },
      }))}
      suggestions={search.options.map(option => ({
        id: option.id,
        label: option.label,
        group: option.header,
        detail: option.countLabel ?? option.hint,
        kind: option.dimension ? KIND[option.dimension] : undefined,
        selected: option.isActive,
        muted: option.muted,
        create: option.create,
      }))}
      suggestionsOpen={search.open}
      onSuggestionsOpenChange={search.setOpen}
      highlighted={search.clampedHighlight}
      onHighlightedChange={search.setHighlight}
      onSelectSuggestion={search.selectOption}
      onKeyDown={search.handleKeyDown}
      clear={{ label: t('history.composite.clearAll'), onClick: () => search.clearAll() }}
      suggestionsFooter={t('history.composite.keyboardHint')}
      inputRef={inputRef}
    />
  )
}
