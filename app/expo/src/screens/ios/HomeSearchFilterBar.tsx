import React from 'react';
import { StyleSheet } from 'react-native';
import {
  Button as SwiftUIButton,
  HStack,
  Host,
  Image,
  Label,
  Menu,
  Picker,
  Spacer,
  Text as SwiftUIText,
} from '@expo/ui/swift-ui';
import {
  fixedSize,
  font,
  foregroundStyle,
  frame,
  glassEffect,
  padding,
  pickerStyle,
  tag,
} from '@expo/ui/swift-ui/modifiers';
import type { SFSymbol } from 'sf-symbols-typescript';
import { useTranslation } from 'react-i18next';
import { iosAccent, iosOnAccent, iosSystemHex } from '@/theme/iosDesignTokens';
import { getDisplayKindLabel, type DisplayKind } from '@/utils/displayKind';
import {
  getHistoryDateFilterLabel,
  getHistoryFilterDateOptions,
  getHistoryFilterSourceOptions,
  getHistorySourceFilterLabel,
  HISTORY_FILTER_KIND_OPTIONS,
} from '@/utils/historyFilterOptions';
import type { HistoryDateFilter, HistorySourceFilter } from '@/utils/historyFilters';
import type { HomeController } from '../useHomeController';

export const SEARCH_FILTER_BAR_HEIGHT = 36;

const KIND_SYMBOL: Record<DisplayKind, SFSymbol> = {
  text: 'text.alignleft',
  url: 'link',
  image: 'photo',
  file: 'doc',
  group: 'square.stack',
};
const SOURCE_SYMBOL: Record<Exclude<HistorySourceFilter, 'all'>, SFSymbol> = {
  local: 'iphone',
  remote: 'laptopcomputer',
};
const ALL = 'all';

/**
 * 搜索结果视图的固定筛选行(iOS):类型 / 时间 / 设备三个下拉胶囊,各自是原生菜单里的单选
 * Picker(勾选在左侧),首项「全部」即清除该维度;生效的胶囊以墨色高亮。行不随滚动收起,
 * 有筛选生效时尾部显示「清除」(只清筛选,保留关键词)。
 */
export function HomeSearchFilterBar({ c }: { c: HomeController }) {
  const { t } = useTranslation('history');
  const kind = c.selectedFilterKinds[0] ?? null;
  const date = c.selectedDateFilter;
  const source = c.selectedSourceFilter;
  const isDark = c.theme.isDark;

  return (
    <Host testID="history-search-filter-bar" style={styles.host}>
      <HStack spacing={8} modifiers={[frame({ maxWidth: Infinity, height: SEARCH_FILTER_BAR_HEIGHT })]}>
        <FilterMenu
          testID="history-filter-kind"
          label={kind ? getDisplayKindLabel(kind) : t('filter.chip.kind')}
          symbol={kind ? KIND_SYMBOL[kind] : undefined}
          active={kind !== null}
          isDark={isDark}
          selection={kind ?? ALL}
          onChange={(value) => c.handleSelectFilterKind(value === ALL ? null : (value as DisplayKind))}
        >
          <Label title={t('search.allTypes', { ns: 'home' })} systemImage="square.grid.2x2" modifiers={[tag(ALL)]} />
          {HISTORY_FILTER_KIND_OPTIONS.map((option) => (
            <Label
              key={option}
              title={getDisplayKindLabel(option)}
              systemImage={KIND_SYMBOL[option]}
              modifiers={[tag(option)]}
            />
          ))}
        </FilterMenu>
        <FilterMenu
          testID="history-filter-date"
          label={date !== 'all' ? getHistoryDateFilterLabel(date) : t('filter.chip.date')}
          active={date !== 'all'}
          isDark={isDark}
          selection={date}
          onChange={(value) => c.setSelectedDateFilter(value as HistoryDateFilter)}
        >
          {getHistoryFilterDateOptions().map((option) => (
            <SwiftUIText key={option.value} modifiers={[tag(option.value)]}>
              {option.value === 'all' ? t('search.anyTime', { ns: 'home' }) : option.label}
            </SwiftUIText>
          ))}
        </FilterMenu>
        <FilterMenu
          testID="history-filter-source"
          label={source !== 'all' ? getHistorySourceFilterLabel(source) : t('filter.chip.source')}
          active={source !== 'all'}
          isDark={isDark}
          selection={source}
          onChange={(value) => c.setSelectedSourceFilter(value as HistorySourceFilter)}
        >
          {getHistoryFilterSourceOptions().map((option) => (
            <Label
              key={option.value}
              title={option.label}
              systemImage={option.value === 'all' ? 'square.grid.2x2' : SOURCE_SYMBOL[option.value]}
              modifiers={[tag(option.value)]}
            />
          ))}
        </FilterMenu>
        <Spacer />
        {c.hasActiveFilters ? (
          <SwiftUIButton
            testID="history-filter-clear"
            label={t('search.clear', { ns: 'home' })}
            onPress={c.handleClearFilters}
            modifiers={[font({ size: 16, weight: 'medium' }), padding({ horizontal: 6 })]}
          />
        ) : null}
      </HStack>
    </Host>
  );
}

function FilterMenu({
  testID,
  label,
  symbol,
  active,
  isDark,
  selection,
  onChange,
  children,
}: {
  testID: string;
  label: string;
  symbol?: SFSymbol;
  active: boolean;
  isDark: boolean;
  selection: string;
  onChange: (value: string) => void;
  children: React.ReactNode;
}) {
  const ink = isDark ? iosAccent.dark : iosAccent.light;
  const onInk = isDark ? iosOnAccent.dark : iosOnAccent.light;
  const foreground = active ? onInk : isDark ? iosSystemHex.label.dark : iosSystemHex.label.light;
  return (
    <Menu
      testID={testID}
      label={
        <HStack
          spacing={5}
          modifiers={[
            fixedSize(),
            padding({ leading: symbol ? 10 : 14, trailing: 10 }),
            frame({ height: SEARCH_FILTER_BAR_HEIGHT }),
            glassEffect({
              glass: { variant: 'regular', interactive: true, tint: active ? ink : undefined },
              shape: 'capsule',
            }),
          ]}
        >
          {symbol ? <Image systemName={symbol} size={14} color={foreground} /> : null}
          <SwiftUIText modifiers={[font({ size: 15, weight: 'semibold' }), foregroundStyle(foreground)]}>
            {label}
          </SwiftUIText>
          <Image
            systemName="chevron.down"
            size={11}
            color={foreground}
            modifiers={[font({ weight: 'semibold' })]}
          />
        </HStack>
      }
    >
      <Picker selection={selection} onSelectionChange={(value) => onChange(String(value))} modifiers={[pickerStyle('inline')]}>
        {children}
      </Picker>
    </Menu>
  );
}

const styles = StyleSheet.create({
  host: { height: SEARCH_FILTER_BAR_HEIGHT, alignSelf: 'stretch' },
});
