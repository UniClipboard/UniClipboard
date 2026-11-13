import React, { useEffect, useState } from 'react';
import { Image, Modal, Pressable, StyleSheet, Text, View } from 'react-native';
import Animated from 'react-native-reanimated';
import Ionicons from '@expo/vector-icons/Ionicons';
import { useTranslation } from 'react-i18next';
import { useTheme } from '@/hooks/useTheme';
import { useURLMetadata } from '@/hooks/useURLMetadata';
import { useCardContextTransition } from '@/hooks/useCardContextTransition';
import { getDisplayKindColor, getURLWithoutScheme, type DisplayKind } from '@/utils/displayKind';
import { formatFileSize } from '@/utils';
import type { ActionMenuItem } from '@/utils/actionMenuItems';
import type { ClipboardItem } from '@/types/clipboard';
import type { CardContextOverlayProps } from './CardContextOverlay.types';

const MENU_WIDTH = 250;

export function CardContextOverlay({
  item,
  displayKind,
  anchor,
  actionGroups,
  onDismiss,
}: CardContextOverlayProps) {
  if (!item || !displayKind) return null;
  return (
    <OverlayBody
      key={item.profileHash}
      item={item}
      displayKind={displayKind}
      anchor={anchor}
      actionGroups={actionGroups}
      onDismiss={onDismiss}
    />
  );
}

interface OverlayBodyProps extends Omit<CardContextOverlayProps, 'item' | 'displayKind'> {
  item: ClipboardItem;
  displayKind: DisplayKind;
}

function OverlayBody({ item, displayKind, anchor, actionGroups, onDismiss }: OverlayBodyProps) {
  // 命名为 tr:本组件内的 t 是过渡对象(useCardContextTransition)
  const { t: tr } = useTranslation('history');
  const { theme } = useTheme();
  const t = useCardContextTransition(anchor, onDismiss);

  return (
    <Modal
      visible
      transparent
      animationType="none"
      statusBarTranslucent
      navigationBarTranslucent
      onRequestClose={() => t.close()}
    >
      <View ref={t.rootRef} style={StyleSheet.absoluteFill} collapsable={false}>
        <Animated.View
          style={[StyleSheet.absoluteFill, s.scrim, t.scrimStyle]}
          pointerEvents="none"
        />

        <Pressable
          style={StyleSheet.absoluteFill}
          onPress={() => t.close()}
          accessibilityRole="button"
          accessibilityLabel={tr('menu.closeMenu')}
        />

        <Animated.View
          onLayout={t.onStackLayout}
          pointerEvents="box-none"
          style={[
            s.stack,
            {
              top: t.stackTop,
              [t.side]: t.margin,
              alignItems: t.side === 'left' ? 'flex-start' : 'flex-end',
            },
            t.stackStyle,
          ]}
        >
          <CardPreview
            item={item}
            displayKind={displayKind}
            maxWidth={t.previewMaxWidth}
            maxHeight={t.previewMaxHeight}
            theme={theme}
          />
          <ActionMenu
            groups={actionGroups}
            theme={theme}
            onAction={(action) => t.close(action.onPress)}
          />
        </Animated.View>
      </View>
    </Modal>
  );
}

// ─── Preview（按内容类型展开，补上"看全文/大图"） ─────────────────

interface PreviewProps {
  item: ClipboardItem;
  displayKind: DisplayKind;
  maxWidth: number;
  maxHeight: number;
  theme: ReturnType<typeof useTheme>['theme'];
}

function CardPreview(props: PreviewProps) {
  switch (props.displayKind) {
    case 'image':
      return <ImagePreview {...props} />;
    case 'url':
      return <URLPreview {...props} />;
    case 'file':
    case 'group':
      return <FilePreview {...props} />;
    default:
      return <TextPreview {...props} />;
  }
}

function TextPreview({ item, maxWidth, maxHeight, theme }: PreviewProps) {
  return (
    <View
      style={[s.previewCard, { maxWidth, maxHeight, backgroundColor: theme.colors.surfaceLow }]}
    >
      <Text testID="history-preview-text" style={[s.previewText, { color: theme.colors.textPrimary }]} numberOfLines={24}>
        {item.text}
      </Text>
    </View>
  );
}

function URLPreview({ item, maxWidth, theme }: PreviewProps) {
  const url = item.text.trim();
  const metadata = useURLMetadata(url);
  const kindColor = getDisplayKindColor('url');
  const width = Math.min(maxWidth, 300);
  return (
    <View
      style={[
        s.previewCard,
        s.previewNoPadding,
        { width, backgroundColor: theme.colors.surfaceLow },
      ]}
    >
      {metadata?.ogImageUrl ? (
        <Image source={{ uri: metadata.ogImageUrl }} style={s.ogImage} resizeMode="cover" />
      ) : (
        <View style={[s.urlPlaceholder, { backgroundColor: kindColor + '1F' }]}>
          <Ionicons name="globe-outline" size={36} color={kindColor + '66'} />
        </View>
      )}
      <View style={s.urlInfo}>
        {metadata?.title ? (
          <Text style={[s.urlTitle, { color: theme.colors.textPrimary }]} numberOfLines={2}>
            {metadata.title}
          </Text>
        ) : null}
        <Text style={[s.urlText, { color: kindColor }]} numberOfLines={3}>
          {getURLWithoutScheme(url)}
        </Text>
      </View>
    </View>
  );
}

function ImagePreview({ item, maxWidth, maxHeight, theme }: PreviewProps) {
  const uri = item.isLocalFileReady && item.fileUri ? item.fileUri : null;
  const [ratio, setRatio] = useState<number | null>(null);

  useEffect(() => {
    if (!uri) return;
    let alive = true;
    Image.getSize(
      uri,
      (w, h) => {
        if (alive && w > 0 && h > 0) setRatio(w / h);
      },
      () => {
        if (alive) setRatio(1);
      }
    );
    return () => {
      alive = false;
    };
  }, [uri]);

  if (!uri) {
    const kindColor = getDisplayKindColor('image');
    return (
      <View
        style={[
          s.previewCard,
          s.imagePlaceholder,
          {
            width: Math.min(maxWidth, 260),
            height: 180,
            backgroundColor: theme.colors.surfaceLow,
          },
        ]}
      >
        <Ionicons name="image" size={40} color={kindColor + '80'} />
      </View>
    );
  }

  // 宽高按原图比例适配安全区，比例未知时先按卡片的正方形占位
  let width = maxWidth;
  let height = ratio ? width / ratio : width;
  if (height > maxHeight) {
    height = maxHeight;
    width = Math.max(ratio ? height * ratio : height, 140);
  }
  return (
    <View
      style={[
        s.previewCard,
        s.previewNoPadding,
        { width, height, backgroundColor: theme.colors.surfaceLow },
      ]}
    >
      <Image source={{ uri }} style={StyleSheet.absoluteFill} resizeMode="cover" />
    </View>
  );
}

function FilePreview({ item, displayKind, maxWidth, theme }: PreviewProps) {
  const kindColor = getDisplayKindColor(displayKind);
  return (
    <View
      style={[
        s.previewCard,
        s.filePreview,
        { width: Math.min(maxWidth, 240), backgroundColor: theme.colors.surfaceLow },
      ]}
    >
      <Ionicons
        name={displayKind === 'group' ? 'folder' : 'document'}
        size={44}
        color={kindColor}
      />
      <Text style={[s.fileName, { color: theme.colors.textPrimary }]} numberOfLines={2}>
        {item.dataName || item.text}
      </Text>
      {item.size ? (
        <Text style={[s.fileSize, { color: theme.colors.textSecondary }]}>
          {formatFileSize(item.size)}
        </Text>
      ) : null}
    </View>
  );
}

// ─── Menu（M3 surface + 分组 divider，图标在左） ─────────────────

function ActionMenu({
  groups,
  theme,
  onAction,
}: {
  groups: ActionMenuItem[][];
  theme: ReturnType<typeof useTheme>['theme'];
  onAction: (action: ActionMenuItem) => void;
}) {
  return (
    <View style={[s.menu, { backgroundColor: theme.colors.surfaceHigh }]}>
      {groups.map((group, groupIndex) => (
        <React.Fragment key={group[0]?.key ?? groupIndex}>
          {groupIndex > 0 && (
            <View style={[s.groupSeparator, { backgroundColor: theme.colors.separator }]} />
          )}
          {group.map((action) => {
            const color = action.destructive ? theme.colors.error : theme.colors.textPrimary;
            return (
              <Pressable
                key={action.key}
                testID={`history-action-${action.key}`}
                onPress={() => onAction(action)}
                accessibilityRole="button"
                android_ripple={{ color: theme.colors.separator }}
                style={s.row}
              >
                <Ionicons name={action.icon as any} size={20} color={color} />
                <Text style={[s.rowLabel, { color }]}>{action.label}</Text>
              </Pressable>
            );
          })}
        </React.Fragment>
      ))}
    </View>
  );
}

const s = StyleSheet.create({
  scrim: {
    backgroundColor: 'rgba(0,0,0,0.45)',
  },
  stack: {
    position: 'absolute',
    gap: 10,
  },
  previewCard: {
    borderRadius: 16,
    overflow: 'hidden',
    padding: 14,
    elevation: 6,
  },
  previewNoPadding: {
    padding: 0,
  },
  previewText: {
    fontSize: 15,
    lineHeight: 21,
  },
  ogImage: {
    width: '100%',
    aspectRatio: 1.91,
  },
  urlPlaceholder: {
    width: '100%',
    height: 110,
    alignItems: 'center',
    justifyContent: 'center',
  },
  urlInfo: {
    padding: 12,
    gap: 4,
  },
  urlTitle: {
    fontSize: 14,
    fontWeight: '600',
  },
  urlText: {
    fontSize: 12,
  },
  imagePlaceholder: {
    alignItems: 'center',
    justifyContent: 'center',
  },
  filePreview: {
    alignItems: 'center',
    gap: 8,
    paddingVertical: 24,
  },
  fileName: {
    fontSize: 14,
    fontWeight: '500',
    textAlign: 'center',
  },
  fileSize: {
    fontSize: 12,
  },
  menu: {
    width: MENU_WIDTH,
    borderRadius: 16,
    overflow: 'hidden',
    paddingVertical: 6,
    elevation: 8,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 14,
    height: 48,
    paddingHorizontal: 16,
  },
  rowLabel: {
    fontSize: 15,
  },
  groupSeparator: {
    height: StyleSheet.hairlineWidth,
    marginVertical: 6,
    marginHorizontal: 12,
  },
});
