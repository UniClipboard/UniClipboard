import React, { useEffect, useState } from 'react';
import { Image, Modal, PlatformColor, Pressable, StyleSheet, Text, View } from 'react-native';
import Animated from 'react-native-reanimated';
import { BlurView } from 'expo-blur';
import {
  CircleCheck,
  ClipboardType,
  Copy,
  ExternalLink,
  File,
  FileDown,
  FolderOpen,
  Globe,
  Image as ImageIcon,
  ImageDown,
  RotateCcw,
  Send,
  Share2,
  TextCursor,
  Trash2,
  type LucideIcon,
} from 'lucide-react-native';
import { useTranslation } from 'react-i18next';
import { useURLMetadata } from '@/hooks/useURLMetadata';
import { useCardContextTransition } from '@/hooks/useCardContextTransition';
import { iosColors, iosKindTints, hexToRgba } from '@/theme/iosDesignTokens';
import { getURLWithoutScheme, type DisplayKind } from '@/utils/displayKind';
import { formatFileSize } from '@/utils';
import type { ActionMenuItem } from '@/utils/actionMenuItems';
import type { ClipboardItem } from '@/types/clipboard';
import type { CardContextOverlayProps } from './CardContextOverlay.types';

const MENU_WIDTH = 250;

const ACTION_ICONS: Record<string, LucideIcon> = {
  copy: Copy,
  selectText: TextCursor,
  copyPlain: ClipboardType,
  openBrowser: ExternalLink,
  saveImage: ImageDown,
  saveFile: FileDown,
  resend: RotateCcw,
  share: Share2,
  sendTo: Send,
  select: CircleCheck,
  delete: Trash2,
};

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
  const t = useCardContextTransition(anchor, onDismiss);

  return (
    <Modal visible transparent animationType="none" onRequestClose={() => t.close()}>
      <View ref={t.rootRef} style={StyleSheet.absoluteFill} collapsable={false}>
        <Animated.View style={[StyleSheet.absoluteFill, t.scrimStyle]} pointerEvents="none">
          <BlurView intensity={48} tint="systemMaterial" style={StyleSheet.absoluteFill} />
          <View style={[StyleSheet.absoluteFill, s.dim]} />
        </Animated.View>

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
          />
          <ActionMenu groups={actionGroups} onAction={(action) => t.close(action.onPress)} />
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

function TextPreview({ item, maxWidth, maxHeight }: PreviewProps) {
  return (
    <View style={[s.previewCard, { maxWidth, maxHeight }]}>
      <Text testID="history-preview-text" style={[s.previewText, { color: iosColors!.label }]} numberOfLines={24}>
        {item.text}
      </Text>
    </View>
  );
}

function URLPreview({ item, maxWidth }: PreviewProps) {
  const url = item.text.trim();
  const metadata = useURLMetadata(url);
  const width = Math.min(maxWidth, 300);
  return (
    <View style={[s.previewCard, s.previewNoPadding, { width }]}>
      {metadata?.ogImageUrl ? (
        <Image source={{ uri: metadata.ogImageUrl }} style={s.ogImage} resizeMode="cover" />
      ) : (
        <View style={[s.urlPlaceholder, { backgroundColor: hexToRgba(iosKindTints.url, 0.12) }]}>
          <Globe size={36} color={hexToRgba(iosKindTints.url, 0.4)} />
        </View>
      )}
      <View style={s.urlInfo}>
        {metadata?.title ? (
          <Text style={[s.urlTitle, { color: iosColors!.label }]} numberOfLines={2}>
            {metadata.title}
          </Text>
        ) : null}
        <Text style={[s.urlText, { color: iosKindTints.url }]} numberOfLines={3}>
          {getURLWithoutScheme(url)}
        </Text>
      </View>
    </View>
  );
}

function ImagePreview({ item, maxWidth, maxHeight }: PreviewProps) {
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
    return (
      <View
        style={[s.previewCard, s.imagePlaceholder, { width: Math.min(maxWidth, 260), height: 180 }]}
      >
        <ImageIcon size={40} color={hexToRgba(iosKindTints.image, 0.5)} />
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
    <View style={[s.previewCard, s.previewNoPadding, { width, height }]}>
      <Image source={{ uri }} style={StyleSheet.absoluteFill} resizeMode="cover" />
    </View>
  );
}

function FilePreview({ item, displayKind, maxWidth }: PreviewProps) {
  const tint = iosKindTints[displayKind];
  return (
    <View style={[s.previewCard, s.filePreview, { width: Math.min(maxWidth, 240) }]}>
      {displayKind === 'group' ? (
        <FolderOpen size={44} color={tint} />
      ) : (
        <File size={44} color={tint} />
      )}
      <Text style={[s.fileName, { color: iosColors!.label }]} numberOfLines={2}>
        {item.dataName || item.text}
      </Text>
      {item.size ? (
        <Text style={[s.fileSize, { color: iosColors!.secondaryLabel }]}>
          {formatFileSize(item.size)}
        </Text>
      ) : null}
    </View>
  );
}

// ─── Menu（分组 + 材质背景，label 左 / 图标右，贴近原生 UIMenu） ──

function ActionMenu({
  groups,
  onAction,
}: {
  groups: ActionMenuItem[][];
  onAction: (action: ActionMenuItem) => void;
}) {
  return (
    <View style={s.menu}>
      <BlurView intensity={90} tint="systemMaterial" style={StyleSheet.absoluteFill} />
      {groups.map((group, groupIndex) => (
        <React.Fragment key={group[0]?.key ?? groupIndex}>
          {groupIndex > 0 && (
            <View style={[s.groupSeparator, { backgroundColor: iosColors!.tertiarySystemFill }]} />
          )}
          {group.map((action, actionIndex) => {
            const Icon = ACTION_ICONS[action.key] ?? Copy;
            const color = action.destructive ? PlatformColor('systemRed') : iosColors!.label;
            return (
              <React.Fragment key={action.key}>
                {actionIndex > 0 && (
                  <View style={[s.rowSeparator, { backgroundColor: iosColors!.separator }]} />
                )}
                <Pressable
                  testID={`history-action-${action.key}`}
                  onPress={() => onAction(action)}
                  accessibilityRole="button"
                  style={({ pressed }) => [
                    s.row,
                    pressed && { backgroundColor: iosColors!.tertiarySystemFill },
                  ]}
                >
                  <Text style={[s.rowLabel, { color }]}>{action.label}</Text>
                  <Icon size={19} color={color} />
                </Pressable>
              </React.Fragment>
            );
          })}
        </React.Fragment>
      ))}
    </View>
  );
}

const s = StyleSheet.create({
  dim: {
    backgroundColor: 'rgba(0,0,0,0.08)',
  },
  stack: {
    position: 'absolute',
    gap: 10,
  },
  previewCard: {
    borderRadius: 20,
    borderCurve: 'continuous' as any,
    overflow: 'hidden',
    padding: 14,
    backgroundColor: iosColors?.secondarySystemGroupedBackground,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 8 },
    shadowOpacity: 0.18,
    shadowRadius: 24,
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
    borderRadius: 13,
    borderCurve: 'continuous' as any,
    overflow: 'hidden',
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 6 },
    shadowOpacity: 0.15,
    shadowRadius: 18,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    height: 44,
    paddingHorizontal: 16,
  },
  rowLabel: {
    fontSize: 16,
  },
  rowSeparator: {
    height: StyleSheet.hairlineWidth,
  },
  groupSeparator: {
    height: 7,
  },
});
