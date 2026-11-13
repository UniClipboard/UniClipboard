import React, { useEffect, useMemo, useState } from 'react';
import { useHistoryItemText } from '@/hooks/useHistoryItemText';
import { Image, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { Button as SwiftUIButton, Host, Menu, Section } from '@expo/ui/swift-ui';
import {
  ArrowUpRight,
  ChevronLeft,
  Copy,
  Download,
  Ellipsis,
  ExternalLink,
  File,
  FolderDown,
  Image as ImageIcon,
  Laptop,
  Layers,
  Link,
  RotateCcw,
  Send,
  Share,
  Smartphone,
  Star,
  TextCursor,
  Type,
  type LucideIcon,
} from 'lucide-react-native';
import type { SFSymbol } from 'sf-symbols-typescript';
import { GlassContainer } from '@/components/ui';
import { useShellStore } from '@/shell/store';
import { useURLMetadata } from '@/hooks/useURLMetadata';
import type { HomeController } from '@/screens/useHomeController';
import { iosColors, iosKindTints } from '@/theme/iosDesignTokens';
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
import { getExtensionColor, getFileExtension } from '@/utils/fileTypeColor';

/** 底部浮动工具栏高度 */
export const DETAIL_TOOLBAR_HEIGHT = 52;
/** 工具栏底边与安全区底边的距离下限 */
const TOOLBAR_MIN_BOTTOM = 16;

/** 执行后会离开详情页的动作:先关页,再执行(如进入多选)。删除由条目消失自动关页。 */
const CLOSES_PAGE = new Set(['select']);

const KIND_ICON: Record<DisplayKind, LucideIcon> = {
  text: Type,
  url: Link,
  image: ImageIcon,
  file: File,
  group: Layers,
};

const QUICK_ICON: Record<string, LucideIcon> = {
  selectText: TextCursor,
  share: Share,
  sendTo: Send,
  saveImage: Download,
  saveFile: FolderDown,
  openBrowser: ExternalLink,
  resend: RotateCcw,
  copyPlain: Copy,
};

const MENU_SYMBOL: Record<string, SFSymbol> = {
  copyPlain: 'doc.on.doc',
  selectText: 'character.cursor.ibeam',
  openBrowser: 'safari',
  saveImage: 'square.and.arrow.down',
  saveFile: 'folder',
  resend: 'arrow.clockwise',
  share: 'square.and.arrow.up',
  sendTo: 'paperplane',
  select: 'checkmark.circle',
  delete: 'trash',
};

/** 工具栏底边到屏幕底边的距离 */
export function detailToolbarBottom(safeBottom: number): number {
  return Math.max(TOOLBAR_MIN_BOTTOM, safeBottom);
}

interface ClipboardDetailPageProps {
  c: HomeController;
  item: ClipboardItem;
  onClose: () => void;
}

/**
 * iOS 全屏详情页(推入):玻璃顶栏(返回 / 收藏 + 原生「更多」菜单)、来源信息、按类型渲染的
 * 内容区,以及底部浮动玻璃工具栏(类型相关的 3 个动作 + 「复制」主按钮)。
 * 动作与长按菜单同源(`c.makeActionGroups`),可见性判定一致;删除单独成组放在菜单底部。
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
  const metaLine = [formatRelativeTime(item.timestamp), item.size ? formatFileSize(item.size) : null]
    .filter(Boolean)
    .join(' · ');
  const toolbarBottom = detailToolbarBottom(insets.bottom);
  const contentBottom = toolbarBottom + DETAIL_TOOLBAR_HEIGHT + 24;
  const menuGroups = splitDestructive(layout.overflow);
  const KindIcon = KIND_ICON[displayKind];
  const DeviceIcon = item.deviceName ? Laptop : Smartphone;

  return (
    <View style={[styles.page, { backgroundColor: iosColors?.systemGroupedBackground }]}>
      <View style={[styles.header, { paddingTop: insets.top }]}>
        <Pressable
          testID="detail-back"
          onPress={onClose}
          accessibilityRole="button"
          accessibilityLabel={c.t('action.back', { ns: 'common' })}
        >
          <GlassContainer shape="circle" interactive style={styles.circleButton}>
            <ChevronLeft size={24} color={colors.textPrimary} />
          </GlassContainer>
        </Pressable>
        <Text style={[styles.title, { color: colors.textPrimary }]} numberOfLines={1} accessibilityRole="header">
          {getDisplayKindLabel(displayKind)}
        </Text>
        <GlassContainer shape="capsule" interactive style={styles.headerPill}>
          <Pressable
            testID="detail-star"
            onPress={() => void useShellStore.getState().toggleStar(item.profileHash)}
            accessibilityRole="button"
            accessibilityLabel={c.t('detail.star')}
            accessibilityState={{ selected: item.starred }}
            style={styles.pillButton}
          >
            <Star
              size={20}
              color={item.starred ? iosKindTints.file : colors.textPrimary}
              fill={item.starred ? iosKindTints.file : 'none'}
            />
          </Pressable>
          {menuGroups.length > 0 ? (
            <Host style={styles.pillButton}>
              <Menu
                testID="detail-overflow"
                label={
                  <View style={styles.pillButton} accessibilityLabel={c.t('a11y.more')}>
                    <Ellipsis size={20} color={colors.textPrimary} />
                  </View>
                }
              >
                {menuGroups.map((group, index) => (
                  <Section key={index}>
                    {group.map((action) => (
                      <SwiftUIButton
                        key={action.key}
                        testID={`detail-menu-${action.key}`}
                        label={action.label}
                        systemImage={MENU_SYMBOL[action.key] ?? 'circle'}
                        role={action.destructive ? 'destructive' : undefined}
                        onPress={action.onPress}
                      />
                    ))}
                  </Section>
                ))}
              </Menu>
            </Host>
          ) : null}
        </GlassContainer>
      </View>

      <View style={styles.source}>
        <View style={[styles.deviceTile, { backgroundColor: iosColors?.tertiarySystemFill }]}>
          <DeviceIcon size={22} color={colors.textPrimary} />
        </View>
        <View style={styles.sourceText}>
          <Text style={[styles.sourceTitle, { color: colors.textPrimary }]} numberOfLines={1}>
            {deviceLabel}
          </Text>
          <Text style={[styles.sourceMeta, { color: colors.textSecondary }]} numberOfLines={1}>
            {metaLine}
          </Text>
        </View>
        <View style={[styles.kindChip, { backgroundColor: iosColors?.tertiarySystemFill }]}>
          <KindIcon size={15} strokeWidth={2.2} color={iosKindTints[displayKind]} />
          <Text style={[styles.kindChipText, { color: colors.textPrimary }]}>
            {getDisplayKindLabel(displayKind)}
          </Text>
        </View>
      </View>

      <ScrollView
        style={styles.scroll}
        contentContainerStyle={[styles.content, { paddingBottom: contentBottom }]}
        showsVerticalScrollIndicator={false}
      >
        <DetailContent c={c} item={item} displayKind={displayKind} inline={layout.inline} />
      </ScrollView>

      <View pointerEvents="box-none" style={[styles.toolbarRow, { bottom: toolbarBottom }]}>
        {layout.quick.length > 0 ? (
          <GlassContainer shape="capsule" interactive style={styles.toolbar}>
            {layout.quick.map((action) => {
              const Icon = QUICK_ICON[action.key] ?? Share;
              return (
                <Pressable
                  key={action.key}
                  testID={`detail-quick-${action.key}`}
                  onPress={action.onPress}
                  accessibilityRole="button"
                  accessibilityLabel={action.label}
                  style={styles.toolbarButton}
                >
                  <Icon size={22} color={colors.textPrimary} />
                </Pressable>
              );
            })}
          </GlassContainer>
        ) : null}
        {layout.primary ? (
          <Pressable
            testID="detail-copy"
            onPress={layout.primary.onPress}
            accessibilityRole="button"
            style={[styles.primaryButton, { backgroundColor: colors.accent }]}
          >
            <Copy size={20} strokeWidth={2} color={colors.inverseAccent} />
            <Text style={[styles.primaryLabel, { color: colors.inverseAccent }]}>
              {layout.primary.label}
            </Text>
          </Pressable>
        ) : null}
      </View>
    </View>
  );
}

/** 菜单分组:破坏性动作(删除)单独成组放在最后,与原生 UIMenu 的惯例一致 */
function splitDestructive(actions: ActionMenuItem[]): ActionMenuItem[][] {
  const regular = actions.filter((action) => !action.destructive);
  const destructive = actions.filter((action) => action.destructive);
  return [regular, destructive].filter((group) => group.length > 0);
}

interface DetailContentProps {
  c: HomeController;
  item: ClipboardItem;
  displayKind: DisplayKind;
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

function TextContent({ c, item }: DetailContentProps) {
  const { colors } = c.theme;
  const text = useHistoryItemText(item);
  const charCount = useMemo(() => Array.from(text).length, [text]);
  const lineCount = useMemo(() => text.split('\n').length, [text]);
  return (
    <>
      <View style={[styles.card, styles.textCard, { backgroundColor: iosColors?.secondarySystemGroupedBackground }]}>
        <Text selectable style={[styles.bodyText, { color: colors.textPrimary }]}>
          {text}
        </Text>
      </View>
      <View style={styles.statRow}>
        <Text style={[styles.statText, { color: colors.textSecondary }]}>
          {c.t('detail.stats.chars', { count: charCount })}
        </Text>
        <Text style={[styles.statText, { color: colors.textSecondary }]}>
          {c.t('detail.stats.lines', { count: lineCount })}
        </Text>
      </View>
    </>
  );
}

function UrlContent({ c, item, inline }: DetailContentProps) {
  const { colors } = c.theme;
  const url = useHistoryItemText(item).trim();
  const domain = getURLDomain(url);
  const metadata = useURLMetadata(url);
  const gradient = useMemo(() => getDomainGradient(domain), [domain]);
  const [ogFailed, setOgFailed] = useState(false);
  const openAction = inline.openBrowser;
  const showOg = !!metadata?.ogImageUrl && !ogFailed;

  return (
    <>
      <View style={[styles.card, { backgroundColor: iosColors?.secondarySystemGroupedBackground }]}>
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
          <View style={styles.linkRow}>
            <View style={[styles.domainBadge, { backgroundColor: gradient.end }]}>
              <Text style={styles.domainInitial}>{getDomainInitial(domain)}</Text>
            </View>
            <Text style={[styles.domainText, { color: colors.textPrimary }]} numberOfLines={1}>
              {domain}
            </Text>
            {openAction ? (
              <Pressable
                testID="detail-open-browser"
                onPress={openAction.onPress}
                accessibilityRole="button"
                accessibilityLabel={openAction.label}
                style={[styles.tonalButton, { backgroundColor: iosColors?.tertiarySystemFill }]}
              >
                <ArrowUpRight size={17} strokeWidth={2} color={colors.textPrimary} />
                <Text style={[styles.tonalLabel, { color: colors.textPrimary }]} numberOfLines={1}>
                  {openAction.label}
                </Text>
              </Pressable>
            ) : null}
          </View>
          {showOg && metadata?.title ? (
            <Text style={[styles.linkDescription, { color: colors.textSecondary }]} numberOfLines={3}>
              {metadata.title}
            </Text>
          ) : null}
        </View>
      </View>
      <View style={[styles.infoBox, { backgroundColor: iosColors?.secondarySystemGroupedBackground }]}>
        <Text style={[styles.infoBoxLabel, { color: colors.textSecondary }]}>
          {c.t('detail.fullUrl')}
        </Text>
        <Text selectable style={[styles.monoText, { color: colors.textPrimary }]}>
          {url}
        </Text>
      </View>
    </>
  );
}

function ImageContent({ c, item }: DetailContentProps) {
  const { colors } = c.theme;
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
    { key: 'format', value: format ? format.toUpperCase() : '—' },
    { key: 'size', value: item.size ? formatFileSize(item.size) : '—' },
  ];

  return (
    <>
      <View style={[styles.card, styles.imageCard, { backgroundColor: iosColors?.tertiarySystemFill }]}>
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
            <ImageIcon size={44} strokeWidth={1.4} color={colors.textSecondary} />
            <Text style={[styles.statText, { color: colors.textSecondary }]}>
              {c.t('detail.imageUnavailable')}
            </Text>
          </View>
        )}
      </View>
      <View style={[styles.infoStrip, { backgroundColor: iosColors?.secondarySystemGroupedBackground }]}>
        {info.map(({ key, value }, index) => (
          <View
            key={key}
            style={[
              styles.infoCell,
              index > 0 && { borderLeftWidth: StyleSheet.hairlineWidth, borderLeftColor: iosColors?.separator },
            ]}
          >
            <Text style={[styles.infoCellLabel, { color: colors.textSecondary }]}>
              {c.t(`detail.info.${key}`)}
            </Text>
            <Text style={[styles.infoCellValue, { color: colors.textPrimary }]} numberOfLines={1}>
              {value}
            </Text>
          </View>
        ))}
      </View>
    </>
  );
}

function FileContent({ c, item, displayKind }: DetailContentProps) {
  const { colors } = c.theme;
  const name = item.dataName || item.text;
  const isGroup = displayKind === 'group';
  const ext = getFileExtension(name);
  const badgeColor = isGroup ? iosKindTints.group : getExtensionColor(ext);
  const rows = [
    { key: 'fileName', value: name },
    { key: 'fileType', value: ext ? ext.toUpperCase() : '—' },
    {
      key: 'localState',
      value: c.t(item.isLocalFileReady ? 'detail.info.localReady' : 'detail.info.localMissing'),
    },
    { key: 'time', value: new Date(item.timestamp).toLocaleString() },
  ];

  return (
    <>
      <View style={[styles.card, styles.fileHero, { backgroundColor: iosColors?.secondarySystemGroupedBackground }]}>
        <View style={[styles.fileDoc, { borderColor: iosColors?.separator }]}>
          {isGroup ? (
            <Layers size={30} color={badgeColor} />
          ) : (
            <View style={[styles.fileBadge, { backgroundColor: badgeColor }]}>
              <Text style={styles.fileBadgeText}>{ext ? ext.toUpperCase() : '?'}</Text>
            </View>
          )}
        </View>
        <Text selectable style={[styles.fileName, { color: colors.textPrimary }]} numberOfLines={3}>
          {name}
        </Text>
        {item.size ? (
          <Text style={[styles.fileMeta, { color: colors.textSecondary }]}>{formatFileSize(item.size)}</Text>
        ) : null}
      </View>
      <View style={[styles.card, { backgroundColor: iosColors?.secondarySystemGroupedBackground }]}>
        {rows.map(({ key, value }, index) => (
          <View key={key} style={styles.infoRow}>
            {index > 0 ? (
              <View style={[styles.rowSeparator, { backgroundColor: iosColors?.separator }]} />
            ) : null}
            <Text style={[styles.infoRowLabel, { color: colors.textSecondary }]}>
              {c.t(`detail.info.${key}`)}
            </Text>
            <Text
              selectable
              style={[styles.infoRowValue, { color: colors.textPrimary }]}
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
  page: { flex: 1 },
  header: {
    paddingHorizontal: 16,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  circleButton: { width: 44, height: 44, alignItems: 'center', justifyContent: 'center' },
  title: { flex: 1, textAlign: 'center', fontSize: 17, fontWeight: '600' },
  headerPill: { height: 44, paddingHorizontal: 2, flexDirection: 'row', alignItems: 'center' },
  pillButton: { width: 40, height: 40, alignItems: 'center', justifyContent: 'center' },
  source: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 20,
    paddingTop: 12,
    paddingBottom: 14,
  },
  deviceTile: {
    width: 40,
    height: 40,
    borderRadius: 10,
    borderCurve: 'continuous',
    alignItems: 'center',
    justifyContent: 'center',
  },
  sourceText: { flex: 1, minWidth: 0, gap: 1 },
  sourceTitle: { fontSize: 15, lineHeight: 20, fontWeight: '600' },
  sourceMeta: { fontSize: 13, lineHeight: 18 },
  kindChip: {
    height: 28,
    paddingLeft: 8,
    paddingRight: 10,
    borderRadius: 14,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
  },
  kindChipText: { fontSize: 13, fontWeight: '600' },
  scroll: { flex: 1 },
  content: { paddingHorizontal: 16, gap: 12 },
  card: { borderRadius: 22, borderCurve: 'continuous', overflow: 'hidden' },
  textCard: { padding: 18 },
  bodyText: { fontSize: 17, lineHeight: 26 },
  statRow: { flexDirection: 'row', gap: 14, paddingHorizontal: 6 },
  statText: { fontSize: 13 },
  linkHero: { height: 168, padding: 18, justifyContent: 'flex-end' },
  linkHeroTitle: { fontSize: 20, lineHeight: 25, fontWeight: '700', color: '#FFFFFF' },
  linkBody: { paddingHorizontal: 18, paddingTop: 14, paddingBottom: 18, gap: 12 },
  linkRow: { flexDirection: 'row', alignItems: 'center', gap: 10 },
  domainBadge: { width: 28, height: 28, borderRadius: 7, alignItems: 'center', justifyContent: 'center' },
  domainInitial: { fontSize: 13, fontWeight: '700', color: '#FFFFFF' },
  domainText: { flex: 1, fontSize: 15, fontWeight: '600' },
  tonalButton: {
    height: 34,
    paddingLeft: 12,
    paddingRight: 16,
    borderRadius: 17,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    flexShrink: 1,
  },
  tonalLabel: { fontSize: 14, fontWeight: '600' },
  linkDescription: { fontSize: 15, lineHeight: 21 },
  infoBox: { borderRadius: 18, borderCurve: 'continuous', paddingHorizontal: 16, paddingVertical: 12, gap: 4 },
  infoBoxLabel: { fontSize: 13, fontWeight: '600' },
  monoText: { fontFamily: 'Menlo', fontSize: 14, lineHeight: 20 },
  imageCard: { minHeight: 280, alignItems: 'center', justifyContent: 'center' },
  image: { width: '100%', minHeight: 280, maxHeight: 560 },
  imagePlaceholder: { alignItems: 'center', gap: 8, padding: 32 },
  infoStrip: { flexDirection: 'row', borderRadius: 18, borderCurve: 'continuous' },
  infoCell: { flex: 1, paddingHorizontal: 14, paddingVertical: 10, gap: 2 },
  infoCellLabel: { fontSize: 12 },
  infoCellValue: { fontSize: 15, fontWeight: '600' },
  fileHero: { alignItems: 'center', gap: 12, paddingHorizontal: 20, paddingTop: 28, paddingBottom: 22 },
  fileDoc: {
    width: 84,
    height: 104,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
    backgroundColor: '#FFFFFF',
    alignItems: 'center',
    justifyContent: 'flex-end',
    paddingBottom: 14,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 4 },
    shadowOpacity: 0.1,
    shadowRadius: 5,
  },
  fileBadge: { paddingHorizontal: 8, paddingVertical: 3, borderRadius: 6 },
  fileBadgeText: { fontSize: 14, fontWeight: '700', color: '#FFFFFF' },
  fileName: { fontSize: 20, lineHeight: 25, fontWeight: '700', textAlign: 'center' },
  fileMeta: { fontSize: 15 },
  infoRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    gap: 12,
    paddingHorizontal: 16,
    paddingVertical: 13,
  },
  rowSeparator: { position: 'absolute', left: 16, right: 0, top: 0, height: StyleSheet.hairlineWidth },
  infoRowLabel: { fontSize: 15, lineHeight: 20 },
  infoRowValue: { flexShrink: 1, fontSize: 15, lineHeight: 20, textAlign: 'right' },
  toolbarRow: {
    position: 'absolute',
    left: 0,
    right: 0,
    flexDirection: 'row',
    justifyContent: 'center',
    alignItems: 'center',
    gap: 10,
  },
  toolbar: {
    height: DETAIL_TOOLBAR_HEIGHT,
    paddingHorizontal: 4,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 2,
  },
  toolbarButton: { width: 44, height: 44, alignItems: 'center', justifyContent: 'center' },
  primaryButton: {
    height: DETAIL_TOOLBAR_HEIGHT,
    paddingLeft: 18,
    paddingRight: 26,
    borderRadius: DETAIL_TOOLBAR_HEIGHT / 2,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    shadowColor: '#15171C',
    shadowOffset: { width: 0, height: 6 },
    shadowOpacity: 0.22,
    shadowRadius: 8,
  },
  primaryLabel: { fontSize: 17, fontWeight: '600' },
});
