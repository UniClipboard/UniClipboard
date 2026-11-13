import i18n from '@/i18n';
import { DisplayKind } from './displayKind';
import { HistoryDateFilter, HistorySourceFilter } from './historyFilters';

export const HISTORY_FILTER_KIND_OPTIONS: DisplayKind[] = ['text', 'url', 'image', 'file', 'group'];

// 模块级定义 + 稍后渲染:标签必须在调用时经 i18n.t 求值,否则语言切换后不更新
export function getHistoryFilterDateOptions(): Array<{ value: HistoryDateFilter; label: string }> {
  return [
    { value: 'all', label: i18n.t('history:filter.date.all') },
    { value: 'today', label: i18n.t('history:filter.date.today') },
    { value: 'yesterday', label: i18n.t('history:filter.date.yesterday') },
    { value: 'pastWeek', label: i18n.t('history:filter.date.pastWeek') },
  ];
}

export function getHistoryDateFilterLabel(dateFilter: HistoryDateFilter): string {
  const options = getHistoryFilterDateOptions();
  return options.find((option) => option.value === dateFilter)?.label ?? options[0].label;
}

export function getHistoryFilterSourceOptions(): Array<{
  value: HistorySourceFilter;
  label: string;
}> {
  return [
    { value: 'all', label: i18n.t('history:filter.source.all') },
    { value: 'local', label: i18n.t('history:filter.source.local') },
    { value: 'remote', label: i18n.t('history:filter.source.remote') },
  ];
}

export function getHistorySourceFilterLabel(sourceFilter: HistorySourceFilter): string {
  const options = getHistoryFilterSourceOptions();
  return options.find((option) => option.value === sourceFilter)?.label ?? options[0].label;
}
