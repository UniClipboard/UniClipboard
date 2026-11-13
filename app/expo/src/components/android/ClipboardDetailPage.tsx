import React, { useEffect, useMemo, useState } from 'react';
import { Image, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import { useURLMetadata } from '@/hooks/useURLMetadata';
import type { HomeController } from '@/screens/useHomeController';
import type { ColorScheme } from '@/theme/colors';
import { m3Type } from '@/theme/m3Typography';
import type { ClipboardItem } from '@/types/clipboard';
import { formatFileSize } from '@/utils';
import type { ActionMenuItem } from '@/utils/actionMenuItems';
import { getDetailPageActionLayout } from '@/utils/detailActionLayout';

import {
  formatRelativeTime,
  getDisplayKind,
  getDisplayKindLabel,
  getURLDomain,
  type DisplayKind,
} from '@/utils/displayKind';
import { getDomainGradient, getDomainInitial } from '@/utils/domainColor';
import { useHistoryItemText } from '@/hooks/useHistoryItemText';
import { getExtensionColor, getFileExtension } from '@/utils/fileTypeColor';
import {
  DETAIL_TOOLBAR_HEIGHT,
  DetailFloatingToolbar,
  TOOLBAR_MARGIN,
} from './DetailFloatingToolbar';
import { HISTORY_KIND_ICON } from './historyKindStyle';
import { M3IconButton } from './M3IconButton';
import { OverflowMenu } from './OverflowMenu';

/** 执行后会离开详情页的动作:先关页,再执行(如进入多选)。删除由条目消失自动关页。 */
const CLOSES_PAGE = new Set(['select']);

interface ClipboardDetailPageProps {
  c: HomeController;
  item: ClipboardItem;
  onClose: () => void;
}

/**
 * Android 全屏详情页:顶栏(返回 / 类型 / 溢出菜单)、来源信息、按类型渲染的内容区,
 * 以及底部浮动工具栏(类型相关的快捷动作 + 复制)。动作与长按 / 多选入口同源
 * (`c.makeActionGroups`),可见性判定一致。
 */
export function ClipboardDetailPage({ c, item, onClose }: ClipboardDetailPageProps) {
  const { theme, insets } = c;
  const { colors } = theme;
  const displayKind = useMemo(() => getDisplayKind(item.type, item.text), [item]);

  const layout = useMemo(() => {
    const actions = c
      .makeActionGroups(item, displayKind, null)
      .flat()
      .map<ActionMenuItem>((action) =>
        CLOSES_PAGE.has(action.key)
          ? {
              ...action,
              onPress: () => {
                onClose();
                action.onPress();
              },
            }
          : action
      );
    // 「发送到」:经同步通道发给所选设备(与系统分享并存)。发送页盖在详情页之上,返回后仍在详情。
    {
      actions.push({
        key: 'sendTo',
        label: c.t('detail.sendTo'),
        icon: 'paper-plane-outline',
        onPress: () => c.openSendTo(item),
      });
    }
    return getDetailPageActionLayout(actions, displayKind);
  }, [c, item, displayKind, onClose]);

  const deviceLabel = item.deviceName
    ? c.t('detail.fromDevice', { device: item.deviceName })
    : c.t('detail.unknownDevice');
  const metaLine = [
    formatRelativeTime(item.timestamp),
    item.size ? formatFileSize(item.size) : null,
  ]
    .filter(Boolean)
    .join(' · ');
  const contentBottom = insets.bottom + TOOLBAR_MARGIN + DETAIL_TOOLBAR_HEIGHT + 16;

  return (
    <View style={[styles.page, { backgroundColor: colors.background, paddingTop: insets.top }]}>
      <View style={styles.appBar}>
        <M3IconButton
          testID="detail-back"
          icon="arrow-back"
          accessibilityLabel={c.t('action.back', { ns: 'common' })}
          onPress={onClose}
          colors={colors}
          iconColor={colors.textPrimary}
        />
        <Text
          style={[styles.appBarTitle, { color: colors.textPrimary }]}
          numberOfLines={1}
          accessibilityRole="header"
        >
          {getDisplayKindLabel(displayKind)}
        </Text>
        {layout.overflow.length > 0 ? (
          <OverflowMenu testID="detail-overflow" items={layout.overflow} />
        ) : null}
      </View>

      <View style={styles.source}>
        <View style={[styles.deviceAvatar, { backgroundColor: colors.surfaceHighest }]}>
          <Ionicons
            name={item.deviceName ? 'laptop-outline' : 'phone-portrait-outline'}
            size={20}
            color={colors.textSecondary}
          />
        </View>
        <View style={styles.sourceText}>
          <Text style={[m3Type.titleMedium, { color: colors.textPrimary }]} numberOfLines={1}>
            {deviceLabel}
          </Text>
          <Text style={[m3Type.bodyMedium, { color: colors.textSecondary }]} numberOfLines={1}>
            {metaLine}
          </Text>
        </View>
        <View style={[styles.kindChip, { backgroundColor: colors.accentContainer }]}>
          <Ionicons
            name={HISTORY_KIND_ICON[displayKind]}
            size={16}
            color={colors.onAccentContainer}
          />
          <Text style={[m3Type.labelLarge, { color: colors.onAccentContainer }]}>
            {getDisplayKindLabel(displayKind)}
          </Text>
        </View>
      </View>

      <ScrollView
        style={styles.scroll}
        contentContainerStyle={[styles.content, { paddingBottom: contentBottom }]}
        showsVerticalScrollIndicator={false}
      >
        <DetailContent
          c={c}
          item={item}
          displayKind={displayKind}
          colors={colors}
          inline={layout.inline}
        />
      </ScrollView>

      <DetailFloatingToolbar
        primary={layout.primary}
        quick={layout.quick}
        colors={colors}
        bottomInset={insets.bottom}
      />
    </View>
  );
}

interface DetailContentProps {
  c: HomeController;
  item: ClipboardItem;
  displayKind: DisplayKind;
  colors: ColorScheme;
  inline: Record<string, ActionMenuItem>;
}

function DetailContent(props: DetailContentProps) {
  switch (props.displayKind) {
    case 'url':
      return <UrlContent {...props} />;
    case 'image':
      return <ImageContent {...props} />;
    case 'file':
    case 'group':
      return <FileContent {...props} />;
    default:
      return <TextContent {...props} />;
  }
}

function TextContent({ c, item, colors }: DetailContentProps) {
  const text = useHistoryItemText(item);
  const charCount = useMemo(() => Array.from(text).length, [text]);
  const lineCount = useMemo(() => text.split('\n').length, [text]);
  return (
    <>
      <View style={[styles.surfaceCard, styles.textCard, { backgroundColor: colors.surfaceLow }]}>
        <Text selectable style={[styles.bodyText, { color: colors.textPrimary }]}>
          {text}
        </Text>
      </View>
      <View style={styles.statRow}>
        <Text style={[m3Type.bodySmall, { color: colors.textSecondary }]}>
          {c.t('detail.stats.chars', { count: charCount })}
        </Text>
        <Text style={[m3Type.bodySmall, { color: colors.textSecondary }]}>
          {c.t('detail.stats.lines', { count: lineCount })}
        </Text>
      </View>
    </>
  );
}

function UrlContent({ c, item, colors, inline }: DetailContentProps) {
  const url = useHistoryItemText(item).trim();
  const domain = getURLDomain(url);
  const metadata = useURLMetadata(url);
  const gradient = useMemo(() => getDomainGradient(domain), [domain]);
  const [ogFailed, setOgFailed] = useState(false);
  const openAction = inline.openBrowser;
  const showOg = !!metadata?.ogImageUrl && !ogFailed;

  return (
    <>
      <View style={[styles.surfaceCard, { backgroundColor: colors.surfaceLow }]}>
        <View style={[styles.linkHero, { backgroundColor: gradient.start }]}>
          {showOg ? (
            <Image
              source={{ uri: metadata!.ogImageUrl }}
              style={StyleSheet.absoluteFill}
              resizeMode="cover"
              onError={() => setOgFailed(true)}
            />
          ) : (
            <Text style={styles.linkHeroTitle} numberOfLines={3}>
              {metadata?.title || domain}
            </Text>
          )}
        </View>
        <View style={styles.linkBody}>
          {showOg && metadata?.title ? (
            <Text style={[m3Type.titleMedium, { color: colors.textPrimary }]} numberOfLines={2}>
              {metadata.title}
            </Text>
          ) : null}
          <View style={styles.linkRow}>
            <View style={[styles.domainBadge, { backgroundColor: gradient.end }]}>
              <Text style={styles.domainInitial}>{getDomainInitial(domain)}</Text>
            </View>
            <Text
              style={[m3Type.titleSmall, styles.flexFill, { color: colors.textPrimary }]}
              numberOfLines={1}
            >
              {domain}
            </Text>
            {openAction ? (
              <Pressable
                testID="detail-open-browser"
                onPress={openAction.onPress}
                accessibilityRole="button"
                accessibilityLabel={openAction.label}
                android_ripple={{ color: colors.fillSecondary as string }}
                style={[styles.tonalButton, { backgroundColor: colors.surfaceHighest }]}
              >
                <Ionicons name="open-outline" size={18} color={colors.textPrimary} />
                <Text style={[m3Type.labelLarge, { color: colors.textPrimary }]} numberOfLines={1}>
                  {openAction.label}
                </Text>
              </Pressable>
            ) : null}
          </View>
        </View>
      </View>
      <View style={[styles.outlinedBox, { borderColor: colors.separator }]}>
        <Text style={[m3Type.labelMedium, { color: colors.textSecondary }]}>
          {c.t('detail.fullUrl')}
        </Text>
        <Text selectable style={[styles.monoText, { color: colors.textPrimary }]}>
          {url}
        </Text>
      </View>
    </>
  );
}

function ImageContent({ c, item, colors }: DetailContentProps) {
  const uri = item.fileUri && item.isLocalFileReady ? item.fileUri : null;
  const [size, setSize] = useState<{ width: number; height: number } | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setSize(null);
    setFailed(false);
    if (!uri) return;
    let cancelled = false;
    Image.getSize(
      uri,
      (width, height) => {
        if (!cancelled) setSize({ width, height });
      },
      () => {
        if (!cancelled) setFailed(true);
      }
    );
    return () => {
      cancelled = true;
    };
  }, [uri]);

  const format = getFileExtension(item.dataName) || getFileExtension(uri ?? undefined);
  const info = [
    { key: 'dimensions', value: size ? `${size.width} × ${size.height}` : '—' },
    { key: 'format', value: format || '—' },
    { key: 'size', value: item.size ? formatFileSize(item.size) : '—' },
  ];

  return (
    <>
      <View style={[styles.surfaceCard, styles.imageCard, { backgroundColor: colors.surfaceLow }]}>
        {uri && !failed ? (
          <Image
            testID="detail-image"
            source={{ uri }}
            style={[styles.image, size ? { aspectRatio: size.width / size.height } : null]}
            resizeMode="contain"
            onError={() => setFailed(true)}
          />
        ) : (
          <View style={styles.imagePlaceholder}>
            <Ionicons name="image-outline" size={44} color={colors.textSecondary} />
            <Text style={[m3Type.bodyMedium, { color: colors.textSecondary }]}>
              {c.t('detail.imageUnavailable')}
            </Text>
          </View>
        )}
      </View>
      <View style={styles.infoGrid}>
        {info.map(({ key, value }) => (
          <View key={key} style={[styles.infoCell, { backgroundColor: colors.surfaceLow }]}>
            <Text style={[m3Type.labelSmall, { color: colors.textSecondary }]}>
              {c.t(`detail.info.${key}`)}
            </Text>
            <Text style={[m3Type.titleSmall, { color: colors.textPrimary }]} numberOfLines={1}>
              {value}
            </Text>
          </View>
        ))}
      </View>
    </>
  );
}

function FileContent({ c, item, displayKind, colors }: DetailContentProps) {
  const name = item.dataName || item.text;
  const isGroup = displayKind === 'group';
  const ext = getFileExtension(name);
  const badgeColor = isGroup ? '#AF52DE' : getExtensionColor(ext);
  const rows = [
    { key: 'fileName', value: name },
    { key: 'fileType', value: ext || '—' },
    {
      key: 'localState',
      value: c.t(item.isLocalFileReady ? 'detail.info.localReady' : 'detail.info.localMissing'),
    },
    { key: 'time', value: new Date(item.timestamp).toLocaleString() },
  ];

  return (
    <>
      <View style={[styles.surfaceCard, styles.fileHero, { backgroundColor: colors.surfaceLow }]}>
        <View style={[styles.fileBadge, { backgroundColor: badgeColor }]}>
          {isGroup ? (
            <Ionicons name="albums-outline" size={32} color="#FFFFFF" />
          ) : (
            <Text style={styles.fileBadgeText}>{ext || '?'}</Text>
          )}
        </View>
        <Text selectable style={[styles.fileName, { color: colors.textPrimary }]} numberOfLines={3}>
          {name}
        </Text>
        {item.size ? (
          <Text style={[m3Type.bodyMedium, { color: colors.textSecondary }]}>
            {formatFileSize(item.size)}
          </Text>
        ) : null}
      </View>
      <View style={[styles.surfaceCard, styles.infoList, { backgroundColor: colors.surfaceLow }]}>
        {rows.map(({ key, value }, index) => (
          <View
            key={key}
            style={[
              styles.infoRow,
              index > 0 && {
                borderTopWidth: StyleSheet.hairlineWidth,
                borderTopColor: colors.separator,
              },
            ]}
          >
            <Text style={[m3Type.bodyMedium, { color: colors.textSecondary }]}>
              {c.t(`detail.info.${key}`)}
            </Text>
            <Text
              selectable
              style={[m3Type.titleSmall, styles.infoValue, { color: colors.textPrimary }]}
              numberOfLines={2}
            >
              {value}
            </Text>
          </View>
        ))}
      </View>
    </>
  );
}

const styles = StyleSheet.create({
  page: {
    flex: 1,
  },
  appBar: {
    height: 64,
    paddingHorizontal: 4,
    flexDirection: 'row',
    alignItems: 'center',
  },
  appBarTitle: {
    ...m3Type.titleLarge,
    flex: 1,
    paddingLeft: 4,
  },
  source: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 20,
    paddingTop: 4,
    paddingBottom: 16,
  },
  deviceAvatar: {
    width: 40,
    height: 40,
    borderRadius: 20,
    alignItems: 'center',
    justifyContent: 'center',
  },
  sourceText: {
    flex: 1,
    minWidth: 0,
  },
  kindChip: {
    height: 32,
    paddingLeft: 8,
    paddingRight: 12,
    borderRadius: 8,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  scroll: {
    flex: 1,
  },
  content: {
    paddingHorizontal: 16,
    gap: 12,
  },
  surfaceCard: {
    borderRadius: 20,
    overflow: 'hidden',
  },
  textCard: {
    padding: 20,
  },
  bodyText: {
    fontSize: 16,
    lineHeight: 26,
  },
  statRow: {
    flexDirection: 'row',
    gap: 16,
    paddingHorizontal: 4,
  },
  linkHero: {
    height: 168,
    padding: 20,
    justifyContent: 'flex-end',
  },
  linkHeroTitle: {
    ...m3Type.titleLarge,
    color: '#FFFFFF',
  },
  linkBody: {
    padding: 16,
    paddingHorizontal: 20,
    gap: 12,
  },
  linkRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
  },
  domainBadge: {
    width: 28,
    height: 28,
    borderRadius: 8,
    alignItems: 'center',
    justifyContent: 'center',
  },
  domainInitial: {
    ...m3Type.labelLarge,
    color: '#FFFFFF',
  },
  flexFill: {
    flex: 1,
  },
  tonalButton: {
    height: 40,
    paddingHorizontal: 16,
    borderRadius: 20,
    overflow: 'hidden',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    flexShrink: 1,
  },
  outlinedBox: {
    borderWidth: 1,
    borderRadius: 16,
    paddingHorizontal: 16,
    paddingVertical: 14,
    gap: 6,
  },
  monoText: {
    fontFamily: 'monospace',
    fontSize: 14,
    lineHeight: 21,
  },
  imageCard: {
    minHeight: 240,
    alignItems: 'center',
    justifyContent: 'center',
  },
  image: {
    width: '100%',
    minHeight: 240,
    maxHeight: 520,
  },
  imagePlaceholder: {
    alignItems: 'center',
    gap: 8,
    padding: 32,
  },
  infoGrid: {
    flexDirection: 'row',
    gap: 8,
  },
  infoCell: {
    flex: 1,
    borderRadius: 14,
    paddingHorizontal: 12,
    paddingVertical: 10,
    gap: 2,
  },
  fileHero: {
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 20,
    paddingTop: 32,
    paddingBottom: 24,
  },
  fileBadge: {
    width: 88,
    height: 108,
    borderRadius: 14,
    alignItems: 'center',
    justifyContent: 'flex-end',
    paddingBottom: 14,
  },
  fileBadgeText: {
    fontSize: 18,
    fontWeight: '700',
    color: '#FFFFFF',
  },
  fileName: {
    ...m3Type.titleLarge,
    fontWeight: '500',
    textAlign: 'center',
  },
  infoList: {
    paddingVertical: 4,
  },
  infoRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    gap: 12,
    paddingHorizontal: 20,
    paddingVertical: 12,
  },
  infoValue: {
    flexShrink: 1,
    textAlign: 'right',
  },
});
