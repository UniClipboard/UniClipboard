/**
 * Top Right Menu Component
 * 右上角菜单组件 - 用于首页和历史记录页面
 */

import React, { useState, useRef, useCallback } from 'react';
import {
  View,
  Text,
  TouchableOpacity,
  StyleSheet,
  Modal,
  Pressable,
  Platform,
  ActionSheetIOS,
} from 'react-native';
import { MoreVertical, ChevronRight } from 'react-native-feather';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography, elevation } from '@/theme';

export interface MenuItemConfig {
  label: string;
  onPress?: () => void;
  icon?: React.ReactNode;
  color?: string;
  destructive?: boolean;
  disabled?: boolean;
  submenu?: MenuItemConfig[];
}

interface TopRightMenuProps {
  items: MenuItemConfig[];
  onClose?: () => void;
}

export const TopRightMenu: React.FC<TopRightMenuProps> = ({ items, onClose }) => {
  const { theme } = useTheme();
  const [showMenu, setShowMenu] = useState(false);
  const [submenuItems, setSubmenuItems] = useState<MenuItemConfig[] | null>(null);
  const [menuTopOffset, setMenuTopOffset] = useState(60);
  const menuButtonRef = useRef<React.ComponentRef<typeof TouchableOpacity>>(null);

  const handleOpenMenu = useCallback(() => {
    if (Platform.OS === 'ios') {
      const options = [
        '取消',
        ...items.map((item) => (item.submenu ? `${item.label} ▸` : item.label)),
      ];
      const cancelButtonIndex = 0;

      ActionSheetIOS.showActionSheetWithOptions(
        {
          options,
          cancelButtonIndex,
          destructiveButtonIndex: items.findIndex((item) => item.destructive) + 1,
          tintColor: theme.colors.primary,
        },
        (buttonIndex) => {
          if (buttonIndex > 0 && buttonIndex <= items.length) {
            const item = items[buttonIndex - 1];
            if (item.submenu) {
              const submenuOptions = ['返回', ...item.submenu.map((sub) => sub.label)];
              ActionSheetIOS.showActionSheetWithOptions(
                {
                  options: submenuOptions,
                  cancelButtonIndex: 0,
                  tintColor: theme.colors.primary,
                },
                (subIndex) => {
                  if (subIndex > 0 && subIndex <= item.submenu!.length) {
                    item.submenu![subIndex - 1].onPress?.();
                    onClose?.();
                  }
                }
              );
            } else {
              item.onPress?.();
              onClose?.();
            }
          }
        }
      );
    } else {
      if (menuButtonRef.current) {
        menuButtonRef.current.measure(
          (_x: number, _y: number, _w: number, h: number, _pageX: number, pageY: number) => {
            setMenuTopOffset(pageY + h + 4);
            setShowMenu(true);
          }
        );
      } else {
        setShowMenu(true);
      }
    }
  }, [items, theme.colors.primary, onClose]);

  const handleMenuItemPress = (item: MenuItemConfig) => {
    if (item.disabled) return;
    if (item.submenu) {
      setSubmenuItems(item.submenu);
    } else {
      item.onPress?.();
      setShowMenu(false);
      onClose?.();
    }
  };

  const handleSubmenuPress = (item: MenuItemConfig) => {
    if (item.disabled) return;
    item.onPress?.();
    setShowMenu(false);
    setSubmenuItems(null);
    onClose?.();
  };

  const handleCloseMenu = () => {
    setShowMenu(false);
    setSubmenuItems(null);
    onClose?.();
  };

  const renderMenuItem = (item: MenuItemConfig, index: number, totalItems: number) => (
    <View key={index}>
      <TouchableOpacity
        style={styles.menuItem}
        onPress={() => handleMenuItemPress(item)}
        disabled={item.disabled}
      >
        <Text
          style={[
            styles.menuItemText,
            {
              color: item.disabled
                ? theme.colors.textTertiary
                : item.color ||
                  (item.destructive ? theme.colors.error || '#F44336' : theme.colors.text),
            },
          ]}
        >
          {item.label}
        </Text>
        {item.submenu && <ChevronRight color={theme.colors.textSecondary} width={16} height={16} />}
        {item.icon && !item.disabled && !item.submenu && (
          <View style={styles.menuItemIcon}>{item.icon}</View>
        )}
      </TouchableOpacity>
      {index < totalItems - 1 && (
        <View style={[styles.menuDivider, { backgroundColor: theme.colors.border }]} />
      )}
    </View>
  );

  const renderSubmenuItem = (item: MenuItemConfig, index: number, totalItems: number) => (
    <View key={index}>
      <TouchableOpacity
        style={styles.menuItem}
        onPress={() => handleSubmenuPress(item)}
        disabled={item.disabled}
      >
        <Text
          style={[
            styles.menuItemText,
            {
              color: item.disabled
                ? theme.colors.textTertiary
                : item.color ||
                  (item.destructive ? theme.colors.error || '#F44336' : theme.colors.text),
            },
          ]}
        >
          {item.label}
        </Text>
        {item.icon && !item.disabled && <View style={styles.menuItemIcon}>{item.icon}</View>}
      </TouchableOpacity>
      {index < totalItems - 1 && (
        <View style={[styles.menuDivider, { backgroundColor: theme.colors.border }]} />
      )}
    </View>
  );

  return (
    <>
      <TouchableOpacity
        ref={menuButtonRef}
        onPress={handleOpenMenu}
        style={styles.headerButton}
        hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
      >
        <MoreVertical color={theme.colors.text} width={24} height={24} />
      </TouchableOpacity>

      {Platform.OS === 'android' && (
        <Modal visible={showMenu} transparent animationType="none" onRequestClose={handleCloseMenu}>
          <Pressable style={styles.menuOverlay} onPress={handleCloseMenu}>
            <View
              style={[
                styles.floatingMenu,
                {
                  backgroundColor: theme.colors.surfaceContainerHigh,
                  top: menuTopOffset,
                },
              ]}
            >
              {submenuItems
                ? submenuItems.map((item, index) =>
                    renderSubmenuItem(item, index, submenuItems.length)
                  )
                : items.map((item, index) => renderMenuItem(item, index, items.length))}
            </View>
          </Pressable>
        </Modal>
      )}
    </>
  );
};

const styles = StyleSheet.create({
  headerButton: {
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    justifyContent: 'center',
    alignItems: 'center',
  },
  menuOverlay: {
    flex: 1,
  },
  floatingMenu: {
    position: 'absolute',
    right: spacing.md,
    borderRadius: radius.md,
    borderCurve: 'continuous',
    minWidth: 200,
    overflow: 'hidden',
    paddingVertical: spacing.sm,
    ...elevation.lg,
  },
  menuItem: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingVertical: spacing.md,
    paddingHorizontal: spacing.base,
  },
  menuItemText: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '500',
  },
  menuItemIcon: {
    marginLeft: spacing.sm,
  },
  menuDivider: {
    height: StyleSheet.hairlineWidth,
  },
});
