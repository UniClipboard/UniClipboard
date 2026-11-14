import React from 'react';
import { Keyboard, Pressable, StyleSheet, Text, TextInput, View, type ViewStyle } from 'react-native';
import Animated, { type AnimatedStyle } from 'react-native-reanimated';
import { Search, X, XCircle } from 'lucide-react-native';
import { GlassContainer } from '@/components/ui';
import { FAB_SIZE } from '@/components/AddActionsFab.types';
import type { HomeController } from '../useHomeController';
import { dismissHomeSearch } from './homeSearchSlots';

export function HomeSearchDock({
  c,
  style,
}: {
  c: HomeController;
  /** 下拉关闭时的跟手动画 */
  style?: AnimatedStyle<ViewStyle>;
}) {
  return (
    <Animated.View
      pointerEvents="box-none"
      style={[styles.dock, { bottom: Math.max(0, c.insets.bottom - 4) }, style]}
    >
      <View style={styles.fieldSlot}>
        <GlassContainer shape="capsule" interactive style={styles.field}>
          <Search size={25} color={c.theme.colors.textPrimary} />
          {c.isSearching ? (
            <TextInput
              testID="history-search-input"
              autoFocus
              value={c.searchText}
              onChangeText={c.setSearchText}
              placeholder={c.t('topBar.searchPlaceholder')}
              placeholderTextColor={c.theme.colors.textSecondary}
              accessibilityLabel={c.t('a11y.search')}
              autoCorrect={false}
              autoCapitalize="none"
              returnKeyType="search"
              onSubmitEditing={Keyboard.dismiss}
              style={[styles.input, { color: c.theme.colors.textPrimary }]}
            />
          ) : (
            <Text numberOfLines={1} style={[styles.input, { color: c.theme.colors.textSecondary }]}>
              {c.t('topBar.searchPlaceholder')}
            </Text>
          )}
          {c.isSearching && c.searchText.length > 0 ? (
            <Pressable
              accessibilityRole="button"
              testID="history-search-clear"
              accessibilityLabel={c.t('a11y.clearSearch')}
              onPress={() => c.setSearchText('')}
              style={styles.clear}
            >
              <XCircle size={20} color={c.theme.colors.textSecondary} />
            </Pressable>
          ) : null}
        </GlassContainer>
        {!c.isSearching ? (
          <Pressable
            testID="history-search-open"
            style={StyleSheet.absoluteFill}
            accessibilityRole="button"
            accessibilityLabel={c.t('a11y.search')}
            onPress={c.openSearch}
          />
        ) : null}
      </View>
      {c.isSearching ? (
        <Pressable
          accessibilityRole="button"
          testID="history-search-close"
          accessibilityLabel={c.t('action.cancel', { ns: 'common' })}
          onPress={() => dismissHomeSearch(c.closeSearch)}
        >
          <GlassContainer shape="circle" interactive style={styles.action}>
            <X size={24} color={c.theme.colors.textPrimary} />
          </GlassContainer>
        </Pressable>
      ) : (
        <View style={styles.action} pointerEvents="none" />
      )}
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  dock: {
    position: 'absolute',
    left: 20,
    right: 16,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 16,
  },
  fieldSlot: { flex: 1, minWidth: 0 },
  field: {
    height: FAB_SIZE,
    flexDirection: 'row',
    alignItems: 'center',
    paddingLeft: 18,
    paddingRight: 10,
    gap: 12,
  },
  input: { flex: 1, minWidth: 0, fontSize: 20, padding: 0 },
  action: { width: FAB_SIZE, height: FAB_SIZE, justifyContent: 'center', alignItems: 'center' },
  clear: { width: 36, height: 44, justifyContent: 'center', alignItems: 'center' },
});
