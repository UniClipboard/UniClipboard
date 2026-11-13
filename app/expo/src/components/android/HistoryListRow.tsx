import React, { useCallback, useMemo, useRef, useState } from 'react';
import { Image, Pressable, StyleSheet, Text, View, type ViewStyle } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, { useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import Ionicons from '@expo/vector-icons/Ionicons';
import * as Haptics from 'expo-haptics';
import { useTranslation } from 'react-i18next';
import type { CardAnchorRect } from '@/components/CardContextOverlay.types';
import { useCopyFeedback } from '@/hooks/useCopyFeedback';
import { useDoubleTap } from '@/hooks/useDoubleTap';
import { useTheme } from '@/hooks/useTheme';
import { useURLMetadata } from '@/hooks/useURLMetadata';
import { m3Type } from '@/theme/m3Typography';
import type { ClipboardItem } from '@/types/clipboard';
import { formatFileSize } from '@/utils';
import {
  getDisplayKind,
  getDisplayKindLabel,
  getURLWithoutScheme,
  type DisplayKind,
} from '@/utils/displayKind';
import { getFileExtension } from '@/utils/fileTypeColor';
import { formatHistoryRowTime, type HistoryRowPosition } from '@/utils/historyDayGroups';
import { getHistoryKindTone, HISTORY_KIND_ICON } from './historyKindStyle';
import {
  resolveSwipeAction,
  swipeOffset,
  SWIPE_ACTIVATION,
  SWIPE_MAX,
  SWIPE_VERTICAL_SLOP,
  type HistorySwipeAction,
} from './historySwipe';

export type HistoryListDensity = 'comfortable' | 'compact';

export interface HistoryListRowProps {
  item: ClipboardItem;
  density: HistoryListDensity;
  position: HistoryRowPosition;
  isSelected: boolean;
  isSelectMode: boolean;
  onPress: (item: ClipboardItem) => void;
  /** 复制到系统剪贴板(双击 / 右滑 / 读屏「复制」),返回是否成功 */
  onCopy?: (item: ClipboardItem) => Promise<boolean> | void;
  onLongPress: (item: ClipboardItem, anchor: CardAnchorRect | null) => void;
  /** 左滑 / 读屏「删除」 */
  onDelete: (item: ClipboardItem) => void;
}

/** 分段列表的外圆角 / 内圆角 */
const OUTER_RADIUS = 20;
const INNER_RADIUS = 4;
const PREVIEW_RENDER_CHARS = 300;

function cornerRadii(position: HistoryRowPosition): ViewStyle {
  const top = position === 'single' || position === 'first' ? OUTER_RADIUS : INNER_RADIUS;
  const bottom = position === 'single' || position === 'last' ? OUTER_RADIUS : INNER_RADIUS;
  return {
    borderTopLeftRadius: top,
    borderTopRightRadius: top,
    borderBottomLeftRadius: bottom,
    borderBottomRightRadius: bottom,
  };
}

/**
 * 历史分组列表的一行(Android,M3 Expressive 分段列表)。
 *
 * 整行是一个 Pressable,尾部空白同样可点:单击看详情、双击复制、长按进入多选,与网格卡片一致。
 * 右滑复制、左滑删除(删除走控制器的可撤销删除);多选模式下关闭滑动。
 * comfortable:40dp 类型图标 + 两行预览 + `设备 · 时间`,图片带 52dp 缩略图;
 * compact:一行(类型图标 + 单行文本 + 尾部时间),约 52dp 高。
 */
export const HistoryListRow = React.memo(function HistoryListRow({
  item,
  density,
  position,
  isSelected,
  isSelectMode,
  onPress,
  onCopy,
  onLongPress,
  onDelete,
}: HistoryListRowProps) {
  const { theme } = useTheme();
  const { colors } = theme;
  const { t } = useTranslation('home');
  const rowRef = useRef<View>(null);
  const { justCopied, copy } = useCopyFeedback(item, onCopy);
  const handlePress = useDoubleTap(() => onPress(item), onCopy && !isSelectMode ? copy : undefined);
  const displayKind = useMemo(() => getDisplayKind(item.type, item.text), [item.type, item.text]);
  const kindLabel = getDisplayKindLabel(displayKind);
  const content = useRowContent(item, displayKind, kindLabel);
  const time = formatHistoryRowTime(item.timestamp);
  const device = item.deviceName || t('detail.unknownDevice');
  const radii = cornerRadii(position);
  const compact = density === 'compact';

  const handleLongPress = () => {
    const node = rowRef.current;
    if (!node) {
      onLongPress(item, null);
      return;
    }
    node.measureInWindow((x, y, width, height) => {
      onLongPress(item, width > 0 && height > 0 ? { x, y, width, height } : null);
    });
  };

  const handleSwipe = useCallback(
    (action: HistorySwipeAction) => {
      if (action === 'copy') {
        void copy();
        return;
      }
      // 删除后条目立即从列表隐藏;撤销时列表以新 key 重新挂载这一行,位移随之复位
      onDelete(item);
    },
    [copy, onDelete, item]
  );
  const swipe = useRowSwipe(!isSelectMode, handleSwipe);

  const tone = getHistoryKindTone(displayKind, colors);
  const leading = isSelectMode ? (
    <View style={compact ? styles.compactLeading : styles.leading} accessible={false}>
      <Ionicons
        name={isSelected ? 'checkmark-circle' : 'ellipse-outline'}
        size={24}
        color={isSelected ? colors.accent : colors.textSecondary}
      />
    </View>
  ) : compact ? (
    <View style={styles.compactLeading}>
      <Ionicons name={HISTORY_KIND_ICON[displayKind]} size={20} color={tone.content} />
    </View>
  ) : (
    <View style={[styles.leading, { backgroundColor: tone.container }]}>
      <Ionicons name={HISTORY_KIND_ICON[displayKind]} size={20} color={tone.content} />
    </View>
  );

  const copiedLabel = (
    <View style={styles.copied} testID={`history-row-copied-${item.profileHash}`}>
      <Ionicons name="checkmark" size={14} color={colors.accent} />
      <Text style={[m3Type.labelMedium, { color: colors.accent }]}>{t('card.copied')}</Text>
    </View>
  );

  return (
    <View style={[styles.clip, radii]}>
      <Animated.View
        style={[
          styles.action,
          styles.actionStart,
          { backgroundColor: colors.accent },
          swipe.copyStyle,
        ]}
        accessible={false}
      >
        <Ionicons name="copy-outline" size={22} color={colors.onAccent} />
        <Text style={[styles.actionLabel, { color: colors.onAccent }]} numberOfLines={1}>
          {t('action.copy', { ns: 'common' })}
        </Text>
      </Animated.View>
      <Animated.View
        style={[
          styles.action,
          styles.actionEnd,
          { backgroundColor: colors.error },
          swipe.deleteStyle,
        ]}
        accessible={false}
      >
        <Text style={[styles.actionLabel, { color: colors.onError }]} numberOfLines={1}>
          {t('action.delete', { ns: 'common' })}
        </Text>
        <Ionicons name="trash-outline" size={22} color={colors.onError} />
      </Animated.View>
      <GestureDetector gesture={swipe.gesture}>
        <Animated.View style={swipe.rowStyle}>
          <Pressable
            ref={rowRef}
            testID={`history-row-${item.profileHash}`}
            onPress={handlePress}
            onLongPress={handleLongPress}
            delayLongPress={350}
            android_ripple={{ color: colors.fillSecondary as string, foreground: true }}
            accessibilityRole={isSelectMode ? 'checkbox' : 'button'}
            accessibilityLabel={[kindLabel, content.primary, device, time]
              .filter(Boolean)
              .join(', ')}
            accessibilityHint={t(
              isSelectMode
                ? 'a11y.toggleSelection'
                : onCopy
                ? 'a11y.openItemDetail'
                : 'a11y.copyItem'
            )}
            // 双击与滑动对读屏用户不可靠,复制 / 删除以 TalkBack「操作」菜单提供
            accessibilityActions={
              isSelectMode
                ? undefined
                : [
                    ...(onCopy
                      ? [{ name: 'copy', label: t('action.copy', { ns: 'common' }) }]
                      : []),
                    { name: 'delete', label: t('action.delete', { ns: 'common' }) },
                  ]
            }
            onAccessibilityAction={(event) => {
              if (event.nativeEvent.actionName === 'copy') void copy();
              if (event.nativeEvent.actionName === 'delete') onDelete(item);
            }}
            accessibilityState={{
              selected: isSelectMode ? isSelected : undefined,
              checked: isSelectMode ? isSelected : undefined,
            }}
            style={[
              compact ? styles.compactRow : styles.row,
              radii,
              { backgroundColor: isSelected ? colors.accentContainer : colors.surfaceLow },
            ]}
          >
            {leading}
            {compact ? (
              <>
                <Text style={[styles.compactText, { color: colors.textPrimary }]} numberOfLines={1}>
                  {/* 单行呈现时把换行折叠成空格,多行文本不会只剩第一行 */}
                  {content.primary.replace(/\s+/g, ' ')}
                </Text>
                {justCopied ? (
                  copiedLabel
                ) : (
                  <Text style={[m3Type.bodySmall, { color: colors.textSecondary }]}>{time}</Text>
                )}
              </>
            ) : (
              <>
                <View style={styles.body}>
                  <Text
                    style={[styles.preview, { color: colors.textPrimary }]}
                    numberOfLines={content.secondary ? 1 : 2}
                  >
                    {content.primary}
                  </Text>
                  {content.secondary ? (
                    <Text
                      style={[m3Type.bodyMedium, { color: colors.textSecondary }]}
                      numberOfLines={1}
                    >
                      {content.secondary}
                    </Text>
                  ) : null}
                  {justCopied ? (
                    copiedLabel
                  ) : (
                    <Text
                      style={[m3Type.bodySmall, { color: colors.textSecondary }]}
                      numberOfLines={1}
                    >
                      {`${device} · ${time}`}
                    </Text>
                  )}
                </View>
                {displayKind === 'image' ? <RowThumbnail item={item} /> : null}
              </>
            )}
          </Pressable>
        </Animated.View>
      </GestureDetector>
    </View>
  );
});

/**
 * 行的横滑手势:行跟手平移(带阻力),两侧色块随方向显现;越过阈值时给一次触感提示,
 * 松手时只按实际位移判定动作(见 historySwipe),未过阈值弹回。
 */
function useRowSwipe(enabled: boolean, onSwipe: (action: HistorySwipeAction) => void) {
  const offset = useSharedValue(0);
  const armed = useSharedValue(false);
  const haptic = useCallback(() => {
    Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Confirm).catch(() => {});
  }, []);

  const gesture = useMemo(
    () =>
      Gesture.Pan()
        .enabled(enabled)
        .activeOffsetX([-SWIPE_ACTIVATION, SWIPE_ACTIVATION])
        .failOffsetY([-SWIPE_VERTICAL_SLOP, SWIPE_VERTICAL_SLOP])
        .onUpdate((e) => {
          offset.value = swipeOffset(e.translationX);
          const nowArmed = resolveSwipeAction(offset.value) !== null;
          if (nowArmed && !armed.value) scheduleOnRN(haptic);
          armed.value = nowArmed;
        })
        .onEnd(() => {
          const action = resolveSwipeAction(offset.value);
          armed.value = false;
          if (action === 'delete') {
            // 行随即从列表隐藏,滑出而不是先弹回
            offset.value = withTiming(-SWIPE_MAX * 4, { duration: 180 });
          } else {
            // 用 timing 而不是 spring:回弹越过 0 会闪出另一侧的色块
            offset.value = withTiming(0, { duration: 180 });
          }
          if (action) scheduleOnRN(onSwipe, action);
        })
        .onFinalize(() => {
          armed.value = false;
        }),
    [enabled, offset, armed, haptic, onSwipe]
  );

  const rowStyle = useAnimatedStyle(() => ({ transform: [{ translateX: offset.value }] }));
  const copyStyle = useAnimatedStyle(() => ({ opacity: offset.value > 0 ? 1 : 0 }));
  const deleteStyle = useAnimatedStyle(() => ({ opacity: offset.value < 0 ? 1 : 0 }));
  return { gesture, rowStyle, copyStyle, deleteStyle };
}

/** 行的主文本与可选第二行:有第二行(链接地址、文件大小、归档信息)时主文本收为一行 */
function useRowContent(
  item: ClipboardItem,
  kind: DisplayKind,
  kindLabel: string
): { primary: string; secondary?: string } {
  const metadata = useURLMetadata(kind === 'url' ? item.text.trim() : null);
  const size = item.size ? formatFileSize(item.size) : undefined;
  switch (kind) {
    case 'url': {
      const url = getURLWithoutScheme(item.text);
      return metadata?.title ? { primary: metadata.title, secondary: url } : { primary: url };
    }
    case 'image':
      return { primary: item.dataName || kindLabel, secondary: size };
    case 'file': {
      const name = item.dataName || item.text;
      const ext = getFileExtension(name).toUpperCase();
      return { primary: name, secondary: [size, ext].filter(Boolean).join(' · ') || undefined };
    }
    case 'group':
      return {
        primary: item.dataName || item.text || kindLabel,
        secondary: [kindLabel, size].filter(Boolean).join(' · '),
      };
    case 'text':
    default:
      // 超出两行的文本对排版无贡献,截断以免超长剪贴内容拖慢布局
      return { primary: item.text.trim().slice(0, PREVIEW_RENDER_CHARS) };
  }
}

function RowThumbnail({ item }: { item: ClipboardItem }) {
  const { theme } = useTheme();
  const [failed, setFailed] = useState(false);
  const ready = item.isLocalFileReady && item.fileUri && !failed;
  return (
    <View style={[styles.thumbnail, { backgroundColor: theme.colors.surfaceHighest }]}>
      {ready ? (
        <Image
          source={{ uri: item.fileUri }}
          style={StyleSheet.absoluteFill}
          resizeMode="cover"
          onError={() => setFailed(true)}
        />
      ) : (
        <Ionicons name="image-outline" size={20} color={theme.colors.textSecondary} />
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  clip: { overflow: 'hidden' },
  row: {
    minHeight: 76,
    paddingHorizontal: 16,
    paddingVertical: 12,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 16,
    overflow: 'hidden',
  },
  compactRow: {
    minHeight: 52,
    paddingHorizontal: 16,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 16,
    overflow: 'hidden',
  },
  leading: {
    width: 40,
    height: 40,
    borderRadius: 12,
    alignItems: 'center',
    justifyContent: 'center',
  },
  compactLeading: { alignItems: 'center', justifyContent: 'center' },
  body: { flex: 1, minWidth: 0, gap: 2 },
  preview: { fontSize: 15, lineHeight: 21 },
  compactText: { flex: 1, minWidth: 0, fontSize: 15, lineHeight: 21 },
  thumbnail: {
    width: 52,
    height: 52,
    borderRadius: 10,
    overflow: 'hidden',
    alignItems: 'center',
    justifyContent: 'center',
  },
  copied: { flexDirection: 'row', alignItems: 'center', gap: 4 },
  action: {
    ...StyleSheet.absoluteFill,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  actionStart: { paddingLeft: 20 },
  actionEnd: { justifyContent: 'flex-end', paddingRight: 20 },
  actionLabel: { ...m3Type.labelLarge },
});
