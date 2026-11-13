import React, { useCallback, useRef, useState } from 'react';
import { Modal, Pressable, StyleSheet, Text, View, useWindowDimensions } from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import { useTranslation } from 'react-i18next';
import { useTheme } from '@/hooks/useTheme';
import { m3Type } from '@/theme/m3Typography';
import type { ActionMenuItem } from '@/utils/actionMenuItems';
import { M3IconButton } from './M3IconButton';

/** 菜单项:动作项沿用 ActionMenuItem;选择型菜单可省略图标、用 `selected` 标出当前值。 */
export type OverflowMenuItem = Omit<ActionMenuItem, 'icon'> & {
  icon?: string;
  selected?: boolean;
};

interface OverflowMenuProps {
  items: OverflowMenuItem[];
  testID?: string;
  /** 自定义触发器(如筛选 chip);默认是 48dp ⋮ 图标按钮。 */
  renderTrigger?: (open: () => void) => React.ReactNode;
  /** 菜单与触发器哪条边对齐:行尾的 ⋮ 默认对齐右缘(end);行首的筛选 chip 对齐左缘(start)。 */
  align?: 'start' | 'end';
  /** 选择型菜单的小标题(如「显示方式」),位于首项之上 */
  title?: string;
}

const MENU_MIN_WIDTH = 200;
const MENU_MAX_WIDTH = 280;

/**
 * M3 溢出菜单(⋮)。触发器默认为 48dp 图标按钮;菜单锚在触发器下方、右缘对齐,
 * 行高 48、前导图标 24、无行间分隔线、按压走 ripple,与 Compose DropdownMenu 一致。
 * 选择型菜单(如搜索筛选)在当前项尾部显示对勾。
 */
export function OverflowMenu({
  items,
  testID,
  renderTrigger,
  align = 'end',
  title,
}: OverflowMenuProps) {
  const { theme } = useTheme();
  const { colors } = theme;
  const { t } = useTranslation('common');
  const { width: windowWidth } = useWindowDimensions();
  const anchorRef = useRef<View>(null);
  const [anchor, setAnchor] = useState<
    { top: number; right: number } | { top: number; left: number } | null
  >(null);

  const open = useCallback(() => {
    anchorRef.current?.measureInWindow((x, y, w, h) => {
      setAnchor(
        align === 'start'
          ? { top: y + h, left: Math.max(8, Math.min(x, windowWidth - MENU_MIN_WIDTH - 8)) }
          : { top: y + h, right: Math.max(8, windowWidth - (x + w)) }
      );
    });
  }, [align, windowWidth]);
  const close = useCallback(() => setAnchor(null), []);

  return (
    <>
      <View ref={anchorRef} collapsable={false}>
        {renderTrigger ? (
          renderTrigger(open)
        ) : (
          <M3IconButton
            testID={testID}
            icon="ellipsis-vertical"
            accessibilityLabel={t('action.more')}
            onPress={open}
            colors={colors}
          />
        )}
      </View>

      <Modal visible={anchor != null} transparent animationType="fade" onRequestClose={close}>
        <Pressable
          style={StyleSheet.absoluteFill}
          onPress={close}
          accessibilityLabel={t('action.close')}
        />
        {anchor ? (
          <View
            accessibilityRole="menu"
            style={[styles.menu, anchor, { backgroundColor: colors.surfaceMid }]}
          >
            {title ? (
              <Text style={[styles.title, { color: colors.textSecondary }]} numberOfLines={1}>
                {title}
              </Text>
            ) : null}
            {items.map((item) => {
              const color = item.destructive ? colors.error : colors.textPrimary;
              return (
                <Pressable
                  key={item.key}
                  testID={`overflow-action-${item.key}`}
                  accessibilityRole="menuitem"
                  accessibilityState={
                    item.selected === undefined ? undefined : { checked: item.selected }
                  }
                  onPress={() => {
                    close();
                    item.onPress();
                  }}
                  android_ripple={{ color: colors.fillSecondary as string }}
                  style={styles.item}
                >
                  {item.icon ? (
                    <Ionicons
                      name={item.icon as keyof typeof Ionicons.glyphMap}
                      size={24}
                      color={item.destructive ? colors.error : colors.textSecondary}
                    />
                  ) : null}
                  <Text style={[styles.label, { color }]} numberOfLines={1}>
                    {item.label}
                  </Text>
                  {item.selected ? (
                    <Ionicons name="checkmark" size={20} color={colors.accent} />
                  ) : null}
                </Pressable>
              );
            })}
          </View>
        ) : null}
      </Modal>
    </>
  );
}

const styles = StyleSheet.create({
  menu: {
    position: 'absolute',
    minWidth: MENU_MIN_WIDTH,
    maxWidth: MENU_MAX_WIDTH,
    paddingVertical: 8,
    borderRadius: 4,
    overflow: 'hidden',
    elevation: 3,
  },
  item: {
    minHeight: 48,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 12,
  },
  title: {
    ...m3Type.labelMedium,
    paddingHorizontal: 12,
    paddingTop: 4,
    paddingBottom: 8,
  },
  label: {
    ...m3Type.bodyLarge,
    flexGrow: 1,
    flexShrink: 1,
  },
});
