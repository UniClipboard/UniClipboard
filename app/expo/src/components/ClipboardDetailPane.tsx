import React, { useEffect, useMemo, useState } from 'react';
import {
  View,
  Text,
  StyleSheet,
  ScrollView,
  Image,
  Pressable,
  type ColorValue,
} from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import Animated, { useAnimatedStyle } from 'react-native-reanimated';
import { getDisplayKind, formatRelativeTime, DisplayKind } from '@/utils/displayKind';
import { formatFileSize } from '@/utils';
import type { ActionMenuItem } from '@/utils/actionMenuItems';
import { getDetailActionLayout } from '@/utils/detailActionLayout';
import type { HomeController } from '@/screens/useHomeController';
import type { ClipboardItem } from '@/types/clipboard';
import { ClipboardDetailActionBar } from './ClipboardDetailActionBar';
import { usePopoverTransition } from './usePopoverTransition';
import { useHistoryItemText } from '@/hooks/useHistoryItemText';

function PaneText({ item, color }: { item: ClipboardItem; color: ColorValue }) {
  const text = useHistoryItemText(item);
  return (
    <Text selectable style={[styles.text, { color }]}>
      {text}
    </Text>
  );
}

const AnimatedPressable = Animated.createAnimatedComponent(Pressable);

const KIND_ICON: Record<DisplayKind, string> = {
  text: 'document-text-outline',
  url: 'link-outline',
  image: 'image-outline',
  file: 'document-outline',
  group: 'albums-outline',
};

/**
 * Expanded 布局的详情面板。展示显式传入的 item,未传时回落到 `c.detailItem`。
 * 动作与长按上下文浮层同源(`c.makeActionGroups`),因此两处入口的可用动作与判定完全一致。
 * 面板本身用 theme.colors 中性 token 着色 —— iOS/Android 各自的色板在 token 层解析,
 * 不在此写 Platform.OS 分支。
 */
export function ClipboardDetailPane({
  c,
  item = c.detailItem,
  onClose,
  active = true,
}: {
  c: HomeController;
  item?: ClipboardItem | null;
  onClose?: () => void;
  active?: boolean;
}) {
  const { theme } = c;
  const { colors } = theme;
  const [overflowOpen, setOverflowOpen] = useState(false);

  const displayKind = useMemo(() => (item ? getDisplayKind(item.type, item.text) : null), [item]);

  const actions = useMemo<ActionMenuItem[]>(() => {
    if (!item || !displayKind) return [];
    return c.makeActionGroups(item, displayKind, null).flat();
  }, [item, displayKind, c]);

  const actionLayout = useMemo(
    () => (displayKind ? getDetailActionLayout(actions, displayKind) : null),
    [actions, displayKind]
  );

  // profileHash 可能在不同条目间重复(见网格去重逻辑),叠加 timestamp 作为身份,
  // 保证在两条 hash 相同但实为不同的条目间切换时,overflow 浮层也会复位。
  useEffect(() => {
    setOverflowOpen(false);
  }, [active, item?.profileHash, item?.timestamp]);

  // 遮罩与操作栏内的 overflow 浮层用同一份 open 状态,各自跑相同时序 → 视觉同步淡入淡出。
  const backdrop = usePopoverTransition(overflowOpen);
  const backdropStyle = useAnimatedStyle(() => ({ opacity: backdrop.progress.value }));

  if (!item || !displayKind) {
    return (
      <View style={[styles.container, styles.placeholder]}>
        <Ionicons name="clipboard-outline" size={44} color={colors.textTertiary} />
        <Text style={[styles.placeholderTitle, { color: colors.textPrimary }]}>
          {c.t('detail.placeholderTitle')}
        </Text>
        <Text style={[styles.placeholderDesc, { color: colors.textSecondary }]}>
          {c.t('detail.placeholderDesc')}
        </Text>
      </View>
    );
  }

  const deviceLabel = item.deviceName
    ? c.t('detail.fromDevice', { device: item.deviceName })
    : c.t('detail.unknownDevice');

  return (
    <View style={styles.container}>
      <View style={styles.header}>
        <View style={[styles.kindChip, { backgroundColor: colors.accentContainer }]}>
          <Ionicons
            name={KIND_ICON[displayKind] as never}
            size={18}
            color={colors.onAccentContainer}
          />
        </View>
        <View style={styles.headerText}>
          <Text style={[styles.headerDevice, { color: colors.textPrimary }]} numberOfLines={1}>
            {deviceLabel}
          </Text>
          <Text style={[styles.headerMeta, { color: colors.textSecondary }]} numberOfLines={1}>
            {formatRelativeTime(item.timestamp)}
            {item.size ? ` · ${formatFileSize(item.size)}` : ''}
          </Text>
        </View>
        {onClose && (
          <Pressable
            onPress={onClose}
            accessibilityRole="button"
            accessibilityLabel={c.t('detail.close')}
            hitSlop={8}
            style={({ pressed }) => [styles.closeButton, pressed && styles.closeButtonPressed]}
          >
            <Ionicons name="close" size={22} color={colors.textSecondary} />
          </Pressable>
        )}
      </View>

      <ScrollView contentContainerStyle={styles.content} showsVerticalScrollIndicator={false}>
        {displayKind === 'image' && item.fileUri && item.isLocalFileReady ? (
          <View style={[styles.preview, { backgroundColor: colors.surface }]}>
            <Image source={{ uri: item.fileUri }} style={styles.image} resizeMode="contain" />
          </View>
        ) : displayKind === 'file' || displayKind === 'group' ? (
          <View style={[styles.preview, styles.filePreview, { backgroundColor: colors.surface }]}>
            <Ionicons name="document-outline" size={40} color={colors.textSecondary} />
            <Text style={[styles.fileName, { color: colors.textPrimary }]} numberOfLines={2}>
              {item.dataName || item.text}
            </Text>
          </View>
        ) : (
          <View style={[styles.preview, { backgroundColor: colors.surface }]}>
            <PaneText item={item} color={colors.textPrimary} />
          </View>
        )}
      </ScrollView>

      {backdrop.mounted ? (
        <AnimatedPressable
          testID="detail-overflow-backdrop"
          onPress={() => setOverflowOpen(false)}
          accessibilityRole="button"
          accessibilityLabel={c.t('action.close', { ns: 'common' })}
          style={[StyleSheet.absoluteFill, styles.overflowBackdrop, backdropStyle]}
        />
      ) : null}

      {actionLayout ? (
        <ClipboardDetailActionBar
          primary={actionLayout.primary}
          quick={actionLayout.quick}
          overflow={actionLayout.overflow}
          quickLabels={{
            selectText: c.t('detail.quickActions.select'),
            openBrowser: c.t('detail.quickActions.open'),
            saveImage: c.t('detail.quickActions.save'),
            saveFile: c.t('detail.quickActions.save'),
          }}
          moreLabel={c.t('action.more', { ns: 'common' })}
          theme={theme}
          popoverOpen={overflowOpen}
          onPopoverOpenChange={setOverflowOpen}
        />
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  placeholder: {
    alignItems: 'center',
    justifyContent: 'center',
    gap: 10,
    paddingHorizontal: 40,
  },
  placeholderTitle: {
    fontSize: 17,
    fontWeight: '600',
  },
  placeholderDesc: {
    fontSize: 14,
    textAlign: 'center',
    lineHeight: 20,
  },
  content: {
    paddingHorizontal: 20,
    paddingTop: 4,
    paddingBottom: 20,
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 20,
    paddingTop: 20,
    paddingBottom: 12,
  },
  kindChip: {
    width: 40,
    height: 40,
    borderRadius: 12,
    alignItems: 'center',
    justifyContent: 'center',
  },
  headerText: {
    flex: 1,
  },
  closeButton: {
    width: 36,
    height: 36,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 18,
  },
  closeButtonPressed: {
    opacity: 0.55,
  },
  headerDevice: {
    fontSize: 16,
    fontWeight: '600',
  },
  headerMeta: {
    fontSize: 13,
    marginTop: 2,
  },
  preview: {
    borderRadius: 14,
    padding: 16,
    minHeight: 120,
  },
  text: {
    fontSize: 15,
    lineHeight: 22,
  },
  image: {
    width: '100%',
    height: 240,
  },
  filePreview: {
    alignItems: 'center',
    justifyContent: 'center',
    gap: 12,
  },
  fileName: {
    fontSize: 15,
    fontWeight: '500',
    textAlign: 'center',
  },
  overflowBackdrop: {
    // 透明层,只为捕获浮层外的点击以关闭它;不压暗——详情常驻在双栏右侧或全屏 Modal 里,
    // 压暗单栏观感突兀。整体随浮层同步淡入淡出(此处只有 zIndex,opacity 由动画驱动)。
    zIndex: 2,
  },
});
