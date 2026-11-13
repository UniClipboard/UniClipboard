import { View, Text, TextInput, StyleSheet, Pressable, Keyboard } from 'react-native';
import { useTranslation } from 'react-i18next';
import Ionicons from '@expo/vector-icons/Ionicons';
import { M3IconButton } from './android/M3IconButton';
import { OverflowMenu, type OverflowMenuItem } from './android/OverflowMenu';
import type { HistoryLayout } from '@/hooks/useHistoryDisplaySettings';
import type {
  DefaultTopBarProps,
  SearchTopBarProps,
  SelectModeTopBarProps,
} from './HomeTopBar.types';
import { m3Type } from '@/theme/m3Typography';

/**
 * 首页默认态:M3 Search bar。胶囊点按进入搜索,尾部 ⋮ 切换历史显示方式(仅 Compact 布局)。
 * 设置是底部导航的顶级目的地,不在这里放入口;多选由长按卡片进入。
 */
export function DefaultTopBar({
  onSearch,
  historyLayout,
  onHistoryLayoutChange,
  receiveResults,
  theme,
}: DefaultTopBarProps) {
  const { t } = useTranslation('home');
  const { colors } = theme;
  const layoutItems = useHistoryLayoutMenuItems(historyLayout, onHistoryLayoutChange);
  const menuItems = useReceiveResultsMenuItem(receiveResults, layoutItems);
  return (
    <View style={s.row}>
      <View style={[s.searchBar, { backgroundColor: colors.surfaceHigh }]}>
        <Pressable
          testID="history-search-open"
          onPress={onSearch}
          android_ripple={{ color: colors.fillSecondary as string }}
          style={s.searchTrigger}
          accessibilityRole="search"
          accessibilityLabel={t('a11y.search')}
        >
          <Ionicons name="search" size={24} color={colors.textPrimary} />
          <Text style={[s.hint, { color: colors.textSecondary }]} numberOfLines={1}>
            {t('topBar.searchPlaceholder')}
          </Text>
        </Pressable>
        {menuItems ? (
          <OverflowMenu
            testID="history-layout-menu"
            title={layoutItems ? t('layout.title', { ns: 'history' }) : undefined}
            items={menuItems}
          />
        ) : null}
      </View>
    </View>
  );
}

/** 存在接收尝试时在同一个 ⋮ 菜单末尾追加「接收结果」;没有则保持原菜单不变。 */
function useReceiveResultsMenuItem(
  receiveResults: DefaultTopBarProps['receiveResults'],
  layoutItems: OverflowMenuItem[] | null
): OverflowMenuItem[] | null {
  const { t } = useTranslation('home');
  if (!receiveResults) return layoutItems;
  return [
    ...(layoutItems ?? []),
    {
      key: 'receive-results',
      label: t('receive.menuEntry'),
      icon: 'download-outline',
      onPress: receiveResults.onOpen,
    },
  ];
}

const LAYOUT_ICONS: Record<HistoryLayout, string> = {
  list: 'list-outline',
  compact: 'reorder-four-outline',
  grid: 'grid-outline',
};

/** 「显示方式」单选菜单:列表 / 紧凑列表 / 网格,当前项带对勾 */
function useHistoryLayoutMenuItems(
  current: HistoryLayout | undefined,
  onChange: ((layout: HistoryLayout) => void) | undefined
): OverflowMenuItem[] | null {
  const { t } = useTranslation('history');
  if (!current || !onChange) return null;
  return (['list', 'compact', 'grid'] as const).map((layout) => ({
    key: `layout-${layout}`,
    label: t(`layout.${layout}`),
    icon: LAYOUT_ICONS[layout],
    selected: layout === current,
    onPress: () => onChange(layout),
  }));
}

/**
 * 搜索态:同一条胶囊变为输入框,前导返回箭头退出搜索(M3 search view)。筛选行、快捷筛选与
 * 结果数由 Android 首页注入到顶栏下方与网格(screens/android/homeSearchSlots)。
 */
export function SearchTopBar({ searchText, onChangeText, onClose, theme }: SearchTopBarProps) {
  const { t } = useTranslation('home');
  const { colors } = theme;

  return (
    <View style={s.searchWrap}>
      <View style={s.row}>
        <View style={[s.searchBar, { backgroundColor: colors.surfaceHigh }]}>
          <M3IconButton
            testID="history-search-close"
            icon="arrow-back"
            accessibilityLabel={t('action.close', { ns: 'common' })}
            onPress={onClose}
            iconColor={colors.textPrimary}
            colors={colors}
          />
          <TextInput
            testID="history-search-input"
            style={[s.searchInput, { color: colors.textPrimary }]}
            value={searchText}
            onChangeText={onChangeText}
            placeholder={t('topBar.searchPlaceholder')}
            placeholderTextColor={colors.textSecondary}
            cursorColor={colors.accent as string}
            selectionColor={colors.accentContainer as string}
            returnKeyType="search"
            onSubmitEditing={Keyboard.dismiss}
            autoFocus
          />
          {searchText.length > 0 && (
            <M3IconButton
              testID="history-search-clear"
              icon="close"
              accessibilityLabel={t('a11y.clearSearch')}
              onPress={() => onChangeText('')}
              colors={colors}
            />
          )}
        </View>
      </View>
    </View>
  );
}

/**
 * 多选态:M3 上下文操作栏。前导 × 退出,标题为已选数量,尾部全选;
 * 恰好选中一项时追加溢出菜单承载该项的内容类动作(原长按菜单)。
 */
export function SelectModeTopBar({
  count,
  allSelected,
  onSelectAll,
  onDone,
  itemActions,
  theme,
}: SelectModeTopBarProps) {
  const { t } = useTranslation('home');
  const { colors } = theme;
  return (
    <View testID="history-selection-bar" style={s.row}>
      <M3IconButton
        testID="history-selection-close"
        icon="close"
        accessibilityLabel={t('action.close', { ns: 'common' })}
        onPress={onDone}
        iconColor={colors.textPrimary}
        colors={colors}
      />
      <Text
        testID="history-selection-count"
        style={[s.selectCount, { color: colors.textPrimary }]}
        accessibilityLiveRegion="polite"
        numberOfLines={1}
      >
        {t('topBar.selectedCount', { n: count })}
      </Text>
      <M3IconButton
        testID="history-selection-all"
        icon={allSelected ? 'checkbox' : 'checkbox-outline'}
        accessibilityLabel={
          allSelected ? t('topBar.deselectAll') : t('action.selectAll', { ns: 'common' })
        }
        onPress={onSelectAll}
        colors={colors}
      />
      {itemActions && itemActions.length > 0 ? (
        <OverflowMenu testID="history-selection-more" items={itemActions} />
      ) : null}
    </View>
  );
}

const s = StyleSheet.create({
  row: { flexDirection: 'row', alignItems: 'center', minHeight: 64, gap: 4 },
  searchBar: {
    flex: 1,
    height: 56,
    borderRadius: 28,
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 4,
    overflow: 'hidden',
  },
  searchTrigger: {
    flex: 1,
    alignSelf: 'stretch',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 16,
    paddingLeft: 12,
  },
  hint: { ...m3Type.bodyLarge, flexShrink: 1 },
  searchInput: { ...m3Type.bodyLarge, flex: 1, padding: 0, marginLeft: 4 },
  searchWrap: { gap: 4 },
  selectCount: { ...m3Type.titleLarge, flex: 1, marginLeft: 8 },
});
