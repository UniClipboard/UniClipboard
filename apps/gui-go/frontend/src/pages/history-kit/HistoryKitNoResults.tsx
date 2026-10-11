import { useTranslation } from 'react-i18next'
import type { Dimension } from '@/components/history/composite-search/composite-search-model'
import type { Relaxation } from '@/components/history/composite-search/useZeroResultRelaxations'
import { Button, ContentUnavailable } from '@/ui-kit'

/**
 * Nothing matches (design MB3): the macOS "content unavailable" state, offering to drop one filter
 * with the number of items each choice would find. Searching Recently Deleted is not offered: the
 * Engine keeps no deleted entries.
 */
export function HistoryKitNoResults({
  filterCount,
  relaxations,
  onRemove,
}: {
  filterCount: number
  relaxations: Relaxation[] | null
  onRemove: (dimension: Dimension) => void
}) {
  const { t } = useTranslation()
  return (
    <ContentUnavailable
      icon="search"
      title={t('history.kit.noResults')}
      description={
        filterCount > 0
          ? t('history.kit.noResultsFilters', { count: filterCount })
          : t('history.kit.noResultsText')
      }
    >
      {relaxations?.map(({ chip, count }) => (
        <Button
          key={chip.dimension}
          disabled={count === 0}
          onClick={() => onRemove(chip.dimension)}
        >
          {t('history.kit.relaxWithout', { filter: chip.label, count })}
        </Button>
      ))}
    </ContentUnavailable>
  )
}
