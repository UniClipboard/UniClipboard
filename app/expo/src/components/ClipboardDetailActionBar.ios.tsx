import React, { useCallback, useMemo } from 'react';
import { Pressable, StyleSheet, Text, View, type ColorValue } from 'react-native';
import {
  CheckCircle2,
  Copy,
  Download,
  Ellipsis,
  ExternalLink,
  RotateCcw,
  Share2,
  TextCursorInput,
  type LucideIcon,
} from 'lucide-react-native';
import { MenuView, type MenuAction } from '@expo/ui/community/menu';
import type { ClipboardDetailActionBarProps } from './ClipboardDetailActionBar.types';
import { useDetailActionBarBehavior } from './useDetailActionBarBehavior';

// primary / quick 按钮的行内图标(lucide,贴近 SF Symbols)。
const ACTION_ICONS: Record<string, LucideIcon> = {
  copy: Copy,
  selectText: TextCursorInput,
  openBrowser: ExternalLink,
  saveImage: Download,
  saveFile: Download,
  resend: RotateCcw,
  share: Share2,
  select: CheckCircle2,
};

// overflow 走系统 MenuView(原生 UIMenu),条目用 SF Symbol 名。
const MENU_IMAGES: Record<string, MenuAction['image']> = {
  copyPlain: 'doc.on.clipboard',
  selectText: 'selection.pin.in.out',
  openBrowser: 'safari',
  saveImage: 'square.and.arrow.down',
  saveFile: 'square.and.arrow.down',
  resend: 'arrow.clockwise',
  share: 'square.and.arrow.up',
  select: 'checkmark.circle',
  delete: 'trash',
};

export function ClipboardDetailActionBar({
  primary,
  quick,
  overflow,
  quickLabels,
  moreLabel,
  theme,
}: ClipboardDetailActionBarProps) {
  const { colors } = theme;
  const { compact, handleLayout } = useDetailActionBarBehavior();
  const menuActions = useMemo<MenuAction[]>(
    () =>
      overflow.map((action) => ({
        id: action.key,
        title: action.label,
        image: MENU_IMAGES[action.key],
        attributes: action.destructive ? { destructive: true } : undefined,
      })),
    [overflow]
  );

  const handleMenuAction = useCallback(
    (event: { nativeEvent: { event: string } }) => {
      overflow.find((action) => action.key === event.nativeEvent.event)?.onPress();
    },
    [overflow]
  );

  return (
    <View
      onLayout={handleLayout}
      style={[
        styles.container,
        { backgroundColor: colors.surfaceHigh, borderTopColor: colors.separator },
      ]}
    >
      {primary ? (
        <ActionButton
          action={primary}
          color={colors.onAccent}
          backgroundColor={colors.accent}
          primary
        />
      ) : null}

      {quick.map((action) => (
        <ActionButton
          key={action.key}
          action={action}
          color={colors.textPrimary}
          backgroundColor={colors.surfaceHighest}
          showLabel={!compact}
          label={quickLabels[action.key] ?? action.label}
        />
      ))}

      {overflow.length > 0 ? (
        <MenuView actions={menuActions} onPressAction={handleMenuAction} style={styles.menuHost}>
          <View
            style={[styles.secondary, { backgroundColor: colors.surfaceHighest }]}
            accessible
            accessibilityRole="button"
            accessibilityLabel={moreLabel}
          >
            <Ellipsis size={21} color={colors.textPrimary} />
          </View>
        </MenuView>
      ) : null}
    </View>
  );
}

function ActionButton({
  action,
  color,
  backgroundColor,
  primary,
  showLabel,
  label = action.label,
}: {
  action: ClipboardDetailActionBarProps['quick'][number];
  color: ColorValue;
  backgroundColor: ColorValue;
  primary?: boolean;
  showLabel?: boolean;
  label?: string;
}) {
  const Icon = ACTION_ICONS[action.key] ?? CheckCircle2;
  return (
    <Pressable
      onPress={action.onPress}
      accessibilityRole="button"
      accessibilityLabel={action.label}
      style={({ pressed }) => [
        primary ? styles.primary : styles.secondary,
        showLabel && styles.secondaryExpanded,
        { backgroundColor, opacity: pressed ? 0.7 : 1 },
      ]}
    >
      <Icon size={primary ? 19 : 20} color={color as string} />
      {primary || showLabel ? (
        <Text
          style={[primary ? styles.primaryLabel : styles.secondaryLabel, { color }]}
          numberOfLines={1}
        >
          {label}
        </Text>
      ) : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  container: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    minHeight: 74,
    paddingHorizontal: 16,
    paddingVertical: 12,
    borderTopWidth: StyleSheet.hairlineWidth,
  },
  primary: {
    flex: 1,
    minWidth: 104,
    height: 50,
    borderRadius: 14,
    borderCurve: 'continuous',
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    paddingHorizontal: 16,
  },
  primaryLabel: {
    flexShrink: 1,
    fontSize: 16,
    fontWeight: '600',
  },
  secondary: {
    width: 50,
    height: 50,
    borderRadius: 14,
    borderCurve: 'continuous',
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 7,
  },
  secondaryExpanded: {
    width: 94,
    paddingHorizontal: 12,
  },
  secondaryLabel: {
    flexShrink: 1,
    fontSize: 14,
    fontWeight: '500',
  },
  menuHost: {
    width: 50,
    height: 50,
  },
});
