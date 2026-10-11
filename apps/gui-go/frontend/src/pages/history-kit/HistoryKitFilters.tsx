import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import type { SourceOption } from '@/components/history/composite-search/composite-search-model'
import { tagLabel, type SearchTagOption } from '@/lib/search-tags'
import { Select } from '@/ui-kit'

const ANY = 'any'
const TYPES = [Filter.Text, Filter.RichText, Filter.Code, Filter.Link, Filter.Image, Filter.File]
const TIMES: TimeRangePreset[] = [
  'today',
  'yesterday',
  'last_7d',
  'last_30d',
  'this_week',
  'this_month',
]

interface HistoryKitFiltersProps {
  contentFilter: Filter
  sourceFilter: string | null
  tagFilter: string | null
  timeRange: TimeRangePreset
  sourceOptions: SourceOption[]
  tags: SearchTagOption[]
  onContentFilterChange: (value: Filter) => void
  onSourceFilterChange: (value: string | null) => void
  onTagFilterChange: (value: string | null) => void
  onTimeRangeChange: (value: TimeRangePreset) => void
}

/** The filter row under the toolbar (design MA2): Type, Device, Tag and Time pop-ups. */
export function HistoryKitFilters({
  contentFilter,
  sourceFilter,
  tagFilter,
  timeRange,
  sourceOptions,
  tags,
  onContentFilterChange,
  onSourceFilterChange,
  onTagFilterChange,
  onTimeRangeChange,
}: HistoryKitFiltersProps) {
  const { t } = useTranslation()
  const typeValue = TYPES.includes(contentFilter) ? contentFilter : undefined
  return (
    <div className="flex h-9 shrink-0 items-center gap-1.5 overflow-x-auto px-4 [scrollbar-width:none]">
      <Select
        purpose="filter"
        label={t('history.kit.filterType')}
        placeholder={t('history.kit.filterType')}
        value={typeValue ?? ''}
        items={[
          { value: ANY, label: t('history.kit.anyType') },
          ...TYPES.map(type => ({ value: type, label: t(`history.filter.${type}`) })),
        ]}
        onValueChange={value =>
          onContentFilterChange(value === ANY ? Filter.All : (value as Filter))
        }
      />
      <Select
        purpose="filter"
        label={t('history.kit.filterDevice')}
        placeholder={t('history.kit.filterDevice')}
        value={sourceFilter ?? ''}
        items={[
          { value: ANY, label: t('history.source.all') },
          ...sourceOptions.map(option => ({ value: option.id, label: option.name })),
        ]}
        onValueChange={value => onSourceFilterChange(value === ANY ? null : value)}
      />
      <Select
        purpose="filter"
        label={t('history.kit.filterTag')}
        placeholder={t('history.kit.filterTag')}
        value={tagFilter ?? ''}
        items={[
          { value: ANY, label: t('history.kit.anyTag') },
          ...tags.map(tag => ({ value: tag.id, label: `#${tagLabel(tag, t)}` })),
        ]}
        onValueChange={value => onTagFilterChange(value === ANY ? null : value)}
      />
      <Select
        purpose="filter"
        label={t('history.timeRange.label')}
        placeholder={t('history.kit.filterTime')}
        value={timeRange === 'all_time' ? '' : timeRange}
        items={[
          { value: 'all_time', label: t('history.timeRange.all_time') },
          ...TIMES.map(time => ({ value: time, label: t(`history.timeRange.${time}`) })),
        ]}
        onValueChange={value => onTimeRangeChange(value as TimeRangePreset)}
      />
    </div>
  )
}
