import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import {
  DIMENSION_LABEL_KEYS,
  SYNTAX_KEYS,
  type Dimension,
} from '@/components/history/composite-search/composite-search-model'
import { Button, SegmentedControl, Text } from '@/ui-kit'

const OPERATORS: Dimension[] = ['tag', 'source', 'type', 'time', 'extension']

/**
 * The scope bar shown while searching (design MB1/MB2, the place of Finder's search scope): what
 * to search, the result count, and the operators the field understands (a click starts one).
 * "Incl. Deleted" and "Save as Smart View" are not offered: the Engine has neither.
 */
export function HistoryKitScopeBar({
  contentFilter,
  onContentFilterChange,
  results,
  loading,
  onSeedOperator,
}: {
  contentFilter: Filter
  onContentFilterChange: (filter: Filter) => void
  results: number
  loading: boolean
  onSeedOperator: (dimension: Dimension) => void
}) {
  const { t } = useTranslation()
  return (
    <div className="flex shrink-0 flex-col gap-1.5 px-4 pb-2">
      <div className="flex items-center gap-2">
        <SegmentedControl
          label={t('history.kit.searchScope')}
          value={contentFilter === Filter.Favorited ? Filter.Favorited : Filter.All}
          onValueChange={value => onContentFilterChange(value as Filter)}
          items={[
            { value: Filter.All, label: t('history.sidebar.allItems') },
            { value: Filter.Favorited, label: t('history.sidebar.pinned') },
          ]}
        />
        <Text role="caption" tone="secondary">
          {loading
            ? t('clipboard.item.loading')
            : t('history.composite.results', { count: results })}
        </Text>
      </div>
      <div
        className="flex flex-wrap items-center gap-1"
        role="group"
        aria-label={t('history.kit.operators')}
      >
        <Text role="caption" tone="secondary">
          {t('history.kit.operators')}
        </Text>
        {OPERATORS.map(dimension => (
          <Button
            key={dimension}
            size="small"
            variant="subtle"
            tooltip={t(DIMENSION_LABEL_KEYS[dimension])}
            onClick={() => onSeedOperator(dimension)}
          >
            {SYNTAX_KEYS[dimension]}
          </Button>
        ))}
      </div>
    </div>
  )
}
