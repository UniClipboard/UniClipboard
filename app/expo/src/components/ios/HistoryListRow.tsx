import React, { createContext, useCallback, useContext, useMemo, useRef, useState } from 'react';
import { Image, Pressable, StyleSheet, Text, View, type ViewStyle } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  useAnimatedReaction,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  Easing,
  type SharedValue,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import * as Haptics from 'expo-haptics';
import {
  Check,
  Copy,
  Ellipsis,
  File,
  Image as ImageIcon,
  Layers,
  Link,
  Trash2,
  Type,
  type LucideIcon,
} from 'lucide-react-native';
import { useTranslation } from 'react-i18next';
import type { CardAnchorRect } from '@/components/CardContextOverlay.types';
import { useCopyFeedback } from '@/hooks/useCopyFeedback';
import { useDoubleTap } from '@/hooks/useDoubleTap';
import { useTheme } from '@/hooks/useTheme';
import { useURLMetadata } from '@/hooks/useURLMetadata';
import { hexToRgba, iosColors, iosKindTints } from '@/theme/iosDesignTokens';
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
import {
  isSwipeArmed,
  resolveRowSwipe,
  LEADING_ACTION_WIDTH,
  SWIPE_ACTIVATION,
  SWIPE_VERTICAL_SLOP,
  TRAILING_ACTIONS_WIDTH,
  TRAILING_BUTTON_WIDTH,
} from './historyRowSwipe';

export type HistoryListDensity = 'comfortable' | 'compact';

const KIND_ICON: Record<DisplayKind, LucideIcon> = {
  text: Type,
  url: Link,
  image: ImageIcon,
  file: File,
  group: Layers,
};

/** 分组外圆角(iOS 26 分组列表) */
const GROUP_RADIUS = 24;
const PREVIEW_RENDER_CHARS = 300;
const SNAP = { duration: 220, easing: Easing.bezier(0.2, 0.9, 0.1, 1) };
const COPY_FILL = '#15171C';
const MORE_FILL = '#8E8E93';
const DELETE_FILL = '#FF3B30';

/**
 * 同一时刻只让一行保持展开:任一行开始滑动时写入自己的 key,其他已展开的行在 UI 线程上收起。
 */
const OpenRowContext = createContext<SharedValue<string> | null>(null);
export const HistoryOpenRowProvider = OpenRowContext.Provider;

export interface HistoryListRowProps {
  item: ClipboardItem;
  rowKey: string;
  density: HistoryListDensity;
  position: HistoryRowPosition;
  isSelected: boolean;
  isSelectMode: boolean;
  onPress: (item: ClipboardItem) => void;
  /** 复制到系统剪贴板(双击 / 右滑 / 读屏「复制」),返回是否成功 */
  onCopy?: (item: ClipboardItem) => Promise<boolean> | void;
  /** 长按或左滑「更多」:打开该行的上下文菜单 */
  onLongPress: (item: ClipboardItem, anchor: CardAnchorRect | null) => void;
  /** 左滑「删除」 / 全滑删除 / 读屏「删除」 */
  onDelete: (item: ClipboardItem) => void;
}

function cornerRadii(position: HistoryRowPosition): ViewStyle {
  const top = position === 'single' || position === 'first' ? GROUP_RADIUS : 0;
  const bottom = position === 'single' || position === 'last' ? GROUP_RADIUS : 0;
  return {
    borderTopLeftRadius: top,
    borderTopRightRadius: top,
    borderBottomLeftRadius: bottom,
    borderBottomRightRadius: bottom,
  };
}

/**
 * iOS 历史分组列表的一行。整行可点(尾部空白同样):单击看详情、双击复制、长按上下文菜单。
 * 右滑复制;左滑露出「更多 / 删除」,拖到底直接删除。多选模式下关闭滑动,前导图标换成选择圈。
 * comfortable:36pt 类型图标 + 两行预览 + `设备 · 时间`,图片带缩略图;compact:一行 46pt。
 */
export const HistoryListRow = React.memo(function HistoryListRow({
  item,
  rowKey,
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
  const displayKind = useMemo(() => getDisplayKind(item.type, item.text), [item.type, item.text]);
  const kindLabel = getDisplayKindLabel(displayKind);
  const content = useRowContent(item, displayKind, kindLabel);
  const time = formatHistoryRowTime(item.timestamp);
  const device = item.deviceName || t('detail.unknownDevice');
  const compact = density === 'compact';
  const tint = iosKindTints[displayKind];
  const KindIcon = KIND_ICON[displayKind];

  const openContextMenu = useCallback(() => {
    const node = rowRef.current;
    if (!node) {
      onLongPress(item, null);
      return;
    }
    node.measureInWindow((x, y, width, height) => {
      onLongPress(item, width > 0 && height > 0 ? { x, y, width, height } : null);
    });
  }, [item, onLongPress]);

  const swipe = useRowSwipe({
    rowKey,
    enabled: !isSelectMode,
    onCopy: copy,
    onDelete: () => onDelete(item),
  });
  const tap = useDoubleTap(() => onPress(item), onCopy && !isSelectMode ? copy : undefined);
  const handlePress = () => {
    // 已展开时点按只收起,不打开详情
    if (swipe.closeIfOpen()) return;
    tap();
  };

  const leading = isSelectMode ? (
    <View style={compact ? styles.compactLeading : styles.leading} accessible={false}>
      {isSelected ? (
        <View style={[styles.checkOn, { backgroundColor: colors.accent }]}>
          <Check size={14} strokeWidth={3} color={colors.inverseAccent} />
        </View>
      ) : (
        <View style={[styles.checkOff, { borderColor: iosColors?.tertiaryLabel }]} />
      )}
    </View>
  ) : compact ? (
    <View style={styles.compactLeading}>
      <KindIcon size={20} color={tint} />
    </View>
  ) : (
    <View style={[styles.leading, styles.leadingTile, { backgroundColor: hexToRgba(tint, 0.13) }]}>
      <KindIcon size={20} color={tint} />
    </View>
  );

  const copiedLabel = (
    <View style={styles.copied} testID={`history-row-copied-${item.profileHash}`}>
      <Check size={13} strokeWidth={2.6} color={colors.textPrimary} />
      <Text style={[styles.copiedText, { color: colors.textPrimary }]}>{t('card.copied')}</Text>
    </View>
  );

  const showSeparator = position === 'middle' || position === 'last';
  const radii = cornerRadii(position);

  return (
    <View style={[styles.clip, radii]} onLayout={swipe.onLayout}>
      <Animated.View style={[styles.leadingAction, swipe.leadingStyle]} accessible={false}>
        <Copy size={20} color="#FFFFFF" />
        <Text style={styles.actionLabel}>{t('action.copy', { ns: 'common' })}</Text>
      </Animated.View>
      <Animated.View style={[styles.trailingActions, swipe.trailingStyle]}>
        <Pressable
          testID={`history-row-more-${item.profileHash}`}
          accessibilityRole="button"
          accessibilityLabel={t('a11y.more')}
          onPress={() => {
            swipe.close();
            openContextMenu();
          }}
          style={[styles.trailingButton, { backgroundColor: MORE_FILL }]}
        >
          <Ellipsis size={20} color="#FFFFFF" />
          <Text style={styles.actionLabel}>{t('a11y.more')}</Text>
        </Pressable>
        <Animated.View style={[styles.deleteSlot, swipe.deleteStyle]}>
          <Pressable
            testID={`history-row-delete-${item.profileHash}`}
            accessibilityRole="button"
            accessibilityLabel={t('action.delete', { ns: 'common' })}
            onPress={swipe.deleteNow}
            style={[styles.trailingButton, styles.fill, { backgroundColor: DELETE_FILL }]}
          >
            <Trash2 size={20} color="#FFFFFF" />
            <Text style={styles.actionLabel}>{t('action.delete', { ns: 'common' })}</Text>
          </Pressable>
        </Animated.View>
      </Animated.View>
      <GestureDetector gesture={swipe.gesture}>
        <Animated.View style={swipe.rowStyle}>
          <Pressable
            ref={rowRef}
            testID={`history-row-${item.profileHash}`}
            onPress={handlePress}
            onLongPress={isSelectMode ? undefined : openContextMenu}
            delayLongPress={350}
            accessibilityRole={isSelectMode ? 'checkbox' : 'button'}
            accessibilityLabel={[kindLabel, content.primary, device, time].filter(Boolean).join(', ')}
            accessibilityHint={t(isSelectMode ? 'a11y.toggleSelection' : 'a11y.openItemDetail')}
            // 双击与滑动对读屏用户不可靠,复制 / 删除以 VoiceOver「操作」转子提供
            accessibilityActions={
              isSelectMode
                ? undefined
                : [
                    ...(onCopy ? [{ name: 'copy', label: t('action.copy', { ns: 'common' }) }] : []),
                    { name: 'delete', label: t('action.delete', { ns: 'common' }) },
                  ]
            }
            onAccessibilityAction={(event) => {
              if (event.nativeEvent.actionName === 'copy') void copy();
              if (event.nativeEvent.actionName === 'delete') onDelete(item);
            }}
            accessibilityState={{ checked: isSelectMode ? isSelected : undefined }}
            style={({ pressed }) => [
              compact ? styles.compactRow : styles.row,
              {
                backgroundColor: pressed
                  ? iosColors?.tertiarySystemFill
                  : iosColors?.secondarySystemGroupedBackground,
              },
            ]}
          >
            {showSeparator ? (
              <View
                style={[
                  styles.separator,
                  { left: compact ? 46 : 68, backgroundColor: iosColors?.separator },
                ]}
              />
            ) : null}
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
                  <Text style={[styles.compactTime, { color: colors.textSecondary }]}>{time}</Text>
                )}
              </>
            ) : (
              <>
                <View style={styles.body}>
                  <Text
                    style={[
                      styles.preview,
                      displayKind === 'text' && content.monospace && styles.mono,
                      { color: colors.textPrimary },
                    ]}
                    numberOfLines={content.secondary ? 1 : 2}
                  >
                    {content.primary}
                  </Text>
                  {content.secondary ? (
                    <Text style={[styles.secondary, { color: colors.textSecondary }]} numberOfLines={1}>
                      {content.secondary}
                    </Text>
                  ) : null}
                  {justCopied ? (
                    copiedLabel
                  ) : (
                    <Text style={[styles.meta, { color: colors.textSecondary }]} numberOfLines={1}>
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
 * 行的横滑:位移 1:1 跟手,两侧动作区随方向显现;越过触发阈值时给一次触感提示。
 * 松手按 resolveRowSwipe 判定:复制后弹回、停在右侧按钮全露、或滑出删除。
 */
function useRowSwipe({
  rowKey,
  enabled,
  onCopy,
  onDelete,
}: {
  rowKey: string;
  enabled: boolean;
  onCopy: () => void;
  onDelete: () => void;
}) {
  const openRow = useContext(OpenRowContext);
  const offset = useSharedValue(0);
  const start = useSharedValue(0);
  const width = useSharedValue(0);
  const armed = useSharedValue(false);

  const haptic = useCallback(() => {
    Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium).catch(() => {});
  }, []);

  // 其他行开始滑动时收起本行
  useAnimatedReaction(
    () => openRow?.value ?? '',
    (current) => {
      if (current !== rowKey && offset.value !== 0) offset.value = withTiming(0, SNAP);
    },
    [rowKey]
  );

  const deleteNow = useCallback(() => {
    offset.value = withTiming(-Math.max(width.value, 400), { duration: 200 }, () => {
      scheduleOnRN(onDelete);
    });
  }, [offset, width, onDelete]);

  const gesture = useMemo(
    () =>
      Gesture.Pan()
        .enabled(enabled)
        .activeOffsetX([-SWIPE_ACTIVATION, SWIPE_ACTIVATION])
        .failOffsetY([-SWIPE_VERTICAL_SLOP, SWIPE_VERTICAL_SLOP])
        .onStart(() => {
          start.value = offset.value;
          if (openRow) openRow.value = rowKey;
        })
        .onUpdate((event) => {
          offset.value = start.value + event.translationX;
          const nowArmed = isSwipeArmed(offset.value, width.value);
          if (nowArmed && !armed.value) scheduleOnRN(haptic);
          armed.value = nowArmed;
        })
        .onEnd((event) => {
          armed.value = false;
          const outcome = resolveRowSwipe(offset.value, event.velocityX, width.value);
          if (outcome.kind === 'delete') {
            offset.value = withTiming(-Math.max(width.value, 400), { duration: 200 }, () => {
              scheduleOnRN(onDelete);
            });
            return;
          }
          offset.value = withTiming(outcome.kind === 'openTrailing' ? -TRAILING_ACTIONS_WIDTH : 0, SNAP);
          if (outcome.kind === 'copy') scheduleOnRN(onCopy);
        })
        .onFinalize(() => {
          armed.value = false;
        }),
    [enabled, offset, start, width, armed, openRow, rowKey, haptic, onCopy, onDelete]
  );

  const close = useCallback(() => {
    offset.value = withTiming(0, SNAP);
  }, [offset]);
  const closeIfOpen = useCallback(() => {
    if (offset.value === 0) return false;
    offset.value = withTiming(0, SNAP);
    return true;
  }, [offset]);

  const onLayout = useCallback(
    (event: { nativeEvent: { layout: { width: number } } }) => {
      width.value = event.nativeEvent.layout.width;
    },
    [width]
  );

  const rowStyle = useAnimatedStyle(() => ({ transform: [{ translateX: offset.value }] }));
  const leadingStyle = useAnimatedStyle(() => ({
    opacity: offset.value > 0 ? 1 : 0,
    width: Math.max(LEADING_ACTION_WIDTH, offset.value),
  }));
  const trailingStyle = useAnimatedStyle(() => ({
    opacity: offset.value < 0 ? 1 : 0,
    width: Math.max(TRAILING_ACTIONS_WIDTH, -offset.value),
  }));
  // 拖过按钮区后「删除」按钮随之拉宽,占满露出的区域(系统全滑删除的样子)
  const deleteStyle = useAnimatedStyle(() => ({
    width: Math.max(TRAILING_BUTTON_WIDTH, -offset.value - TRAILING_BUTTON_WIDTH),
  }));

  return {
    gesture,
    rowStyle,
    leadingStyle,
    trailingStyle,
    deleteStyle,
    close,
    closeIfOpen,
    deleteNow,
    onLayout,
  };
}

/** 行的主文本与可选第二行:有第二行(链接地址、文件大小、组信息)时主文本收为一行 */
function useRowContent(
  item: ClipboardItem,
  kind: DisplayKind,
  kindLabel: string
): { primary: string; secondary?: string; monospace?: boolean } {
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
    default: {
      // 超出两行的文本对排版无贡献,截断以免超长剪贴内容拖慢布局
      const text = item.text.trim().slice(0, PREVIEW_RENDER_CHARS);
      return { primary: text, monospace: looksLikeCode(text) };
    }
  }
}

/** 命令行 / 代码片段用等宽字体预览 */
function looksLikeCode(text: string): boolean {
  return /^(\$ |export |npm |pnpm |yarn |git |cd |curl |const |let |import )/.test(text);
}

function RowThumbnail({ item }: { item: ClipboardItem }) {
  const { theme } = useTheme();
  const [failed, setFailed] = useState(false);
  const ready = item.isLocalFileReady && item.fileUri && !failed;
  return (
    <View style={[styles.thumbnail, { backgroundColor: iosColors?.tertiarySystemFill }]}>
      {ready ? (
        <Image
          source={{ uri: item.fileUri }}
          style={StyleSheet.absoluteFill}
          resizeMode="cover"
          onError={() => setFailed(true)}
        />
      ) : (
        <ImageIcon size={20} color={theme.colors.textSecondary} />
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  clip: { overflow: 'hidden' },
  fill: { flex: 1 },
  row: {
    minHeight: 74,
    paddingHorizontal: 16,
    paddingVertical: 11,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 14,
  },
  compactRow: {
    minHeight: 46,
    paddingHorizontal: 16,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 14,
  },
  separator: { position: 'absolute', right: 0, top: 0, height: StyleSheet.hairlineWidth },
  leading: { width: 36, height: 36, alignItems: 'center', justifyContent: 'center' },
  leadingTile: { borderRadius: 9, borderCurve: 'continuous' },
  compactLeading: { alignItems: 'center', justifyContent: 'center' },
  checkOn: { width: 22, height: 22, borderRadius: 11, alignItems: 'center', justifyContent: 'center' },
  checkOff: { width: 22, height: 22, borderRadius: 11, borderWidth: 1.5 },
  body: { flex: 1, minWidth: 0, gap: 2 },
  preview: { fontSize: 16, lineHeight: 21 },
  mono: { fontFamily: 'Menlo', fontSize: 14 },
  secondary: { fontSize: 13, lineHeight: 18 },
  meta: { fontSize: 12, lineHeight: 16 },
  compactText: { flex: 1, minWidth: 0, fontSize: 16 },
  compactTime: { fontSize: 13 },
  thumbnail: {
    width: 48,
    height: 48,
    borderRadius: 9,
    overflow: 'hidden',
    alignItems: 'center',
    justifyContent: 'center',
  },
  copied: { flexDirection: 'row', alignItems: 'center', gap: 4 },
  copiedText: { fontSize: 12, fontWeight: '600' },
  leadingAction: {
    position: 'absolute',
    top: 0,
    bottom: 0,
    left: 0,
    backgroundColor: COPY_FILL,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 3,
  },
  trailingActions: {
    position: 'absolute',
    top: 0,
    bottom: 0,
    right: 0,
    flexDirection: 'row',
  },
  trailingButton: {
    width: TRAILING_BUTTON_WIDTH,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 3,
  },
  deleteSlot: { flexDirection: 'row' },
  actionLabel: { fontSize: 13, fontWeight: '600', color: '#FFFFFF' },
});
