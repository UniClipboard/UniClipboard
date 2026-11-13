import React from 'react';
import { View, Text, TextInput, StyleSheet, Pressable, Keyboard } from 'react-native';
import { useTranslation } from 'react-i18next';
import { Check, Ellipsis, Plus, Search, X, XCircle } from 'lucide-react-native';
import {
  Menu,
  Button as SwiftUIButton,
  Host,
  Label,
  Picker,
  Section,
} from '@expo/ui/swift-ui';
import { pickerStyle, tag } from '@expo/ui/swift-ui/modifiers';
import type { SFSymbol } from 'sf-symbols-typescript';
import type { HistoryLayout } from '@/hooks/useHistoryDisplaySettings';
import { GlassContainer } from '@/components/ui';
import { iosDimensions, iosColors } from '@/theme/iosDesignTokens';
import { useLayoutMode } from '@/hooks/useLayoutMode';
import type {
  DefaultTopBarProps,
  SearchTopBarProps,
  SelectModeTopBarProps,
} from './HomeTopBar.types';

export function DefaultTopBar({
  onSelectMode,
  historyLayout,
  onHistoryLayoutChange,
  addActions,
  receiveResults,
  theme,
}: DefaultTopBarProps) {
  const { t } = useTranslation('home');
  const iconColor = theme.colors.textPrimary;
  return (
    <View pointerEvents="box-none" style={s.defaultRow}>
      <GlassContainer shape="capsule" interactive style={s.actionPill}>
        {addActions ? (
          <Host style={s.pillButton}>
            <Menu
              testID="home-add-menu"
              label={
                <View style={s.pillButton} accessibilityLabel={t('a11y.addContent')}>
                  <Plus size={22} color={iconColor} />
                </View>
              }
            >
              <SwiftUIButton
                systemImage="camera"
                label={t('fab.takePhoto')}
                onPress={addActions.onTakePhoto}
              />
              <SwiftUIButton
                systemImage="photo.on.rectangle"
                label={t('fab.pickImage')}
                onPress={addActions.onPickImage}
              />
              <SwiftUIButton
                systemImage="doc"
                label={t('fab.pickFile')}
                onPress={addActions.onPickFile}
              />
              <SwiftUIButton
                systemImage="doc.on.clipboard"
                label={t('fab.uploadClipboard')}
                onPress={addActions.onUploadClipboard}
              />
              <Section>
                <SwiftUIButton
                  systemImage="arrow.triangle.2.circlepath"
                  label={t('fab.syncNow')}
                  onPress={addActions.onSync}
                />
              </Section>
            </Menu>
          </Host>
        ) : null}
        <Host style={s.pillButton}>
          <Menu
            testID="home-menu"
            label={
              <View style={s.pillButton} accessibilityLabel={t('a11y.more')}>
                <Ellipsis size={22} color={iconColor} />
              </View>
            }
          >
            <SwiftUIButton
              testID="home-menu-select"
              systemImage="checkmark.circle"
              label={t('action.select', { ns: 'common' })}
              onPress={onSelectMode}
            />
            {receiveResults ? (
              <SwiftUIButton
                testID="home-menu-receive-results"
                systemImage="arrow.down.doc"
                label={t('receive.menuEntry')}
                onPress={receiveResults.onOpen}
              />
            ) : null}
            {historyLayout && onHistoryLayoutChange ? (
              <Section title={t('layout.title', { ns: 'history' })}>
                <Picker
                  testID="home-menu-layout"
                  selection={historyLayout}
                  onSelectionChange={(value) => onHistoryLayoutChange(value as HistoryLayout)}
                  modifiers={[pickerStyle('inline')]}
                >
                  {HISTORY_LAYOUTS.map((layout) => (
                    <Label
                      key={layout}
                      title={t(`layout.${layout}`, { ns: 'history' })}
                      systemImage={LAYOUT_SYMBOLS[layout]}
                      modifiers={[tag(layout)]}
                    />
                  ))}
                </Picker>
              </Section>
            ) : null}
          </Menu>
        </Host>
      </GlassContainer>
    </View>
  );
}

const HISTORY_LAYOUTS: readonly HistoryLayout[] = ['list', 'compact', 'grid'];
const LAYOUT_SYMBOLS: Record<HistoryLayout, SFSymbol> = {
  list: 'list.bullet',
  compact: 'list.dash',
  grid: 'square.grid.2x2',
};

export function SearchTopBar({
  searchText,
  onChangeText,
  hasActiveFilters,
  resultCount,
  isLoading,
  onReset,
  onClose,
  theme,
}: SearchTopBarProps) {
  const { t } = useTranslation('home');
  const layoutMode = useLayoutMode();

  return (
    <View style={s.searchWrap}>
      <View style={s.row}>
        <View style={s.boxWrap}>
          <SearchFieldFrame theme={theme}>
            <TextInput
              style={[s.searchInput, { color: theme.colors.textPrimary }]}
              value={searchText}
              onChangeText={onChangeText}
              placeholder={t('topBar.searchPlaceholder')}
              placeholderTextColor={theme.colors.textSecondary}
              accessibilityLabel={t('a11y.search')}
              autoCorrect={false}
              autoCapitalize="none"
              returnKeyType="search"
              onSubmitEditing={Keyboard.dismiss}
              autoFocus
            />
            {searchText.length > 0 && (
              <Pressable
                onPress={() => onChangeText('')}
                accessibilityRole="button"
                accessibilityLabel={t('a11y.clearSearch')}
                style={s.clearButton}
              >
                <XCircle size={19} color={theme.colors.textSecondary} />
              </Pressable>
            )}
          </SearchFieldFrame>
        </View>
        <Pressable
          onPress={() => {
            Keyboard.dismiss();
            onClose();
          }}
          accessibilityRole="button"
          accessibilityLabel={t('action.cancel', { ns: 'common' })}
          style={s.moreButton}
        >
          <X size={22} color={theme.colors.textSecondary} />
        </Pressable>
      </View>

      {layoutMode === 'expanded' ? (
        <View style={s.statusRow}>
          <Text
            accessibilityLiveRegion="polite"
            numberOfLines={1}
            style={[s.statusText, { color: theme.colors.textSecondary }]}
          >
            {isLoading ? t('search.loading') : t('search.resultCount', { count: resultCount ?? 0 })}
          </Text>
          {(searchText.length > 0 || hasActiveFilters) && onReset ? (
            <Pressable onPress={onReset} accessibilityRole="button" style={s.resetButton}>
              <Text style={[s.statusText, { color: theme.colors.textPrimary }]}>
                {t('search.reset')}
              </Text>
            </Pressable>
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

function SearchFieldFrame({
  theme,
  children,
}: {
  theme: SearchTopBarProps['theme'];
  children: React.ReactNode;
}) {
  return (
    <View
      style={[
        s.searchField,
        { backgroundColor: iosColors?.tertiarySystemFill ?? theme.colors.surfaceLow },
      ]}
    >
      <Search size={19} color={theme.colors.textSecondary} />
      {children}
    </View>
  );
}

export function SelectModeTopBar({
  count,
  allSelected,
  onSelectAll,
  onDone,
  theme,
}: SelectModeTopBarProps) {
  const { t } = useTranslation('home');
  return (
    <View style={s.selectionRow}>
      <Pressable
        testID="history-select-all"
        onPress={onSelectAll}
        accessibilityRole="button"
        accessibilityState={{ selected: allSelected }}
      >
        <GlassContainer shape="capsule" interactive style={s.selectAllButton}>
          <Text style={[s.selectAllText, { color: theme.colors.textPrimary }]}>
            {allSelected ? t('topBar.deselectAll') : t('action.selectAll', { ns: 'common' })}
          </Text>
        </GlassContainer>
      </Pressable>
      <Text
        numberOfLines={1}
        accessibilityRole="header"
        accessibilityLiveRegion="polite"
        style={[s.selectCount, { color: theme.colors.textPrimary }]}
      >
        {t('topBar.selectedCount', { n: count })}
      </Text>
      <Pressable
        testID="history-select-done"
        onPress={onDone}
        accessibilityRole="button"
        accessibilityLabel={t('action.done', { ns: 'common' })}
        style={[s.doneButton, { backgroundColor: theme.colors.accent }]}
      >
        <Check size={22} color={theme.colors.inverseAccent} strokeWidth={2.4} />
      </Pressable>
    </View>
  );
}

const s = StyleSheet.create({
  defaultRow: { height: 44, flexDirection: 'row', justifyContent: 'flex-end', alignItems: 'center' },
  actionPill: { height: 44, paddingHorizontal: 2, flexDirection: 'row', alignItems: 'center' },
  pillButton: { width: 40, height: 40, justifyContent: 'center', alignItems: 'center' },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    height: 52,
    gap: 10,
  },
  selectionRow: { flexDirection: 'row', alignItems: 'center', height: 44, gap: 8 },
  selectAllButton: { height: 44, paddingHorizontal: 16, justifyContent: 'center' },
  selectAllText: { fontSize: 16, fontWeight: '600' },
  selectCount: { flex: 1, fontSize: 17, fontWeight: '600', textAlign: 'center' },
  doneButton: {
    width: 44,
    height: 44,
    borderRadius: 22,
    justifyContent: 'center',
    alignItems: 'center',
  },
  searchWrap: { gap: 6 },
  boxWrap: { flex: 1, minWidth: 0 },
  searchField: {
    height: 44,
    borderRadius: iosDimensions.surfaceCornerRadius,
    borderCurve: 'continuous',
    flexDirection: 'row',
    alignItems: 'center',
    paddingLeft: 12,
    paddingRight: 4,
    gap: 10,
  },
  searchInput: { flex: 1, minWidth: 0, fontSize: 17, padding: 0 },
  moreButton: { width: 44, height: 44, justifyContent: 'center', alignItems: 'center' },
  searchPlaceholder: { flex: 1, fontSize: 17 },
  clearButton: { width: 44, height: 44, alignItems: 'center', justifyContent: 'center' },
  statusRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    height: 44,
    gap: 12,
  },
  statusText: { fontSize: 13, flexShrink: 1 },
  resetButton: { minHeight: 44, justifyContent: 'center' },
});
