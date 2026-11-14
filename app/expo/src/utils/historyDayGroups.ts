import i18n from '@/i18n';
import type { ClipboardItem } from '@/types/clipboard';
import { formatRelativeTime } from './displayKind';
import { DAY_MS, startOfDay } from './historyFilters';
import { buildOccurrenceKeys } from './occurrenceKeys';

/** 条目在所属日分组里的位置,决定分段列表的圆角(首尾 20、中间 4) */
export type HistoryRowPosition = 'single' | 'first' | 'middle' | 'last';

export type HistoryListRow =
  | { type: 'header'; key: string; dayStart: number }
  | { type: 'item'; key: string; item: ClipboardItem; position: HistoryRowPosition };

/**
 * 把已排好序的历史拍平成「日分组页眉 + 条目」行,供虚拟列表直接渲染。
 *
 * 分组按条目 timestamp 所在的本地自然日(与搜索日期筛选同一口径),只合并相邻的同日条目:
 * 不重排,列表顺序始终等于 store 的顺序。条目 key 经出现序消歧,脏数据里的重复 profileHash
 * 也不会让虚拟列表的 key 冲突。
 */
export function buildHistoryListRows(
  items: ClipboardItem[],
  keyExtractor: (item: ClipboardItem) => string
): HistoryListRow[] {
  const keys = buildOccurrenceKeys(items, keyExtractor);
  const rows: HistoryListRow[] = [];
  let groupStart = -1;
  let currentDay: number | null = null;

  const closeGroup = (endExclusive: number) => {
    if (groupStart < 0) return;
    const count = endExclusive - groupStart;
    for (let i = 0; i < count; i++) {
      const row = rows[rows.length - count + i];
      if (row.type !== 'item') continue;
      row.position =
        count === 1 ? 'single' : i === 0 ? 'first' : i === count - 1 ? 'last' : 'middle';
    }
  };

  items.forEach((item, index) => {
    const day = startOfDay(item.timestamp);
    if (day !== currentDay) {
      closeGroup(index);
      currentDay = day;
      groupStart = index;
      // 同一天被非同日条目隔开(如置顶)时会出现第二个同日页眉,key 需带上起始下标
      rows.push({ type: 'header', key: `day:${day}:${index}`, dayStart: day });
    }
    rows.push({ type: 'item', key: keys[index], item, position: 'single' });
  });
  closeGroup(items.length);
  return rows;
}

/** 日分组页眉:今天 / 昨天 沿用搜索日期筛选的文案,更早的日期按当前语言显示月日(跨年带年份)。 */
export function formatHistoryDayLabel(dayStart: number, now = Date.now()): string {
  const today = startOfDay(now);
  if (dayStart === today) return i18n.t('history:filter.date.today');
  if (dayStart === startOfDay(now - DAY_MS)) return i18n.t('history:filter.date.yesterday');
  const sameYear = new Date(dayStart).getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(i18n.language, {
    year: sameYear ? undefined : 'numeric',
    month: 'long',
    day: 'numeric',
    weekday: 'short',
  }).format(dayStart);
}

/** 行内时间:今天的条目用相对时间(刚刚 / 5 分钟前),更早的条目页眉已给出日期,只显示钟点。 */
export function formatHistoryRowTime(timestamp: number, now = Date.now()): string {
  if (startOfDay(timestamp) === startOfDay(now)) return formatRelativeTime(timestamp);
  return new Intl.DateTimeFormat(i18n.language, { hour: '2-digit', minute: '2-digit' }).format(
    timestamp
  );
}
