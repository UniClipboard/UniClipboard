import React, { useRef, useState } from 'react';
import {
  View,
  Text,
  StyleSheet,
  Image,
  Pressable,
  type StyleProp,
  type ViewStyle,
} from 'react-native';
import Animated, { useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated';
import Svg, {
  Defs,
  LinearGradient as SvgLinearGradient,
  Stop,
  Rect as SvgRect,
  Pattern as SvgPattern,
} from 'react-native-svg';
import { Check, Image as ImageIcon } from 'lucide-react-native';
import { useTranslation } from 'react-i18next';
import { useTheme } from '@/hooks/useTheme';
import { useURLMetadata } from '@/hooks/useURLMetadata';
import { useCopyFeedback } from '@/hooks/useCopyFeedback';
import { useDoubleTap } from '@/hooks/useDoubleTap';
import { useClipboardCardViewModel } from '@/hooks/useClipboardCardViewModel';
import { ClipboardItem } from '@/types/clipboard';
import {
  iosColors,
  iosAccentColor,
  iosOnAccentColor,
  iosCardShadow,
  iosDimensions,
  iosKindTints,
  hexToRgba,
} from '@/theme/iosDesignTokens';
import { getURLDomain, getURLWithoutScheme, type DisplayKind } from '@/utils/displayKind';
import { getDomainGradient, getDomainInitial, type DomainGradient } from '@/utils/domainColor';
import { getFileExtension, getExtensionColor, stripExtension } from '@/utils/fileTypeColor';
import { formatFileSize } from '@/utils';
import type { ClipboardCardProps } from './ClipboardCard.types';

export const ClipboardCard: React.FC<ClipboardCardProps> = React.memo(
  ({ item, isLatest, isSelected, isSelectMode, onPress, onDoublePress, onLongPress, surfaceColor }) => {
    const { t } = useTranslation('home');
    const { displayKind, kindLabel, relativeTime } = useClipboardCardViewModel(item);
    const { justCopied, copy } = useCopyFeedback(item, onDoublePress);
    // 单击看详情、双击复制;多选模式下单击只切换选中,不识别双击,也就没有等待延迟
    const handlePress = useDoubleTap(
      () => onPress(item),
      onDoublePress && !isSelectMode ? copy : undefined
    );
    const kindColor = iosKindTints[displayKind];

    // 传入 surfaceColor = 双栏「凹陷」样式:卡片比面板暗，此时去掉下投阴影,否则凹陷块反显凸起。
    // compact(iPhone)不传 → 保留阴影，卡片浮起于页面之上。
    const recessed = surfaceColor != null;

    // 按压时轻微收缩，预告"长按有戏"；长按触发后由浮层接管，pressOut 回弹
    const cardRef = useRef<View>(null);
    const pressScale = useSharedValue(1);
    const pressStyle = useAnimatedStyle(() => ({
      transform: [{ scale: pressScale.value }],
    }));

    const handleLongPress = () => {
      if (!onLongPress) return;
      const node = cardRef.current;
      if (!node) {
        onLongPress(item, null);
        return;
      }
      node.measureInWindow((x, y, width, height) => {
        onLongPress(item, width > 0 && height > 0 ? { x, y, width, height } : null);
      });
    };

    return (
      <Animated.View style={[styles.fill, pressStyle]}>
        <Pressable
          ref={cardRef}
          testID={`history-card-${item.profileHash}`}
          onPress={handlePress}
          onLongPress={handleLongPress}
          delayLongPress={350}
          onPressIn={() => {
            pressScale.value = withTiming(0.97, { duration: 180 });
          }}
          onPressOut={() => {
            pressScale.value = withTiming(1, { duration: 150 });
          }}
          style={[
            styles.card,
            !recessed && iosCardShadow,
            {
              backgroundColor: surfaceColor ?? iosColors!.secondarySystemGroupedBackground,
              borderColor: isSelected ? iosAccentColor : 'transparent',
              borderWidth: isSelected ? 2.5 : 0,
            },
          ]}
        >
          <CardBody
            item={item}
            displayKind={displayKind}
            kindLabel={kindLabel}
            kindColor={kindColor}
            relativeTime={relativeTime}
            isLatest={isLatest}
            surfaceColor={surfaceColor}
          />
          {justCopied && !isSelectMode && (
            <View
              testID={`history-card-copied-${item.profileHash}`}
              style={[styles.copiedBadge, { backgroundColor: iosAccentColor }]}
              accessible={false}
            >
              <Check size={13} color={iosOnAccentColor} strokeWidth={2.6} />
              <Text style={[styles.copiedBadgeText, { color: iosOnAccentColor }]}>
                {t('card.copied')}
              </Text>
            </View>
          )}
          {isSelectMode && (
            <View style={styles.selectIndicator} accessible={false}>
              {isSelected ? (
                <View style={[styles.checkBadge, { backgroundColor: iosAccentColor }]}>
                  <Check size={14} color={iosOnAccentColor} strokeWidth={3} />
                </View>
              ) : (
                <View style={styles.uncheckedBadge} />
              )}
            </View>
          )}
        </Pressable>
      </Animated.View>
    );
  }
);

interface CardBodyProps {
  item: ClipboardItem;
  displayKind: DisplayKind;
  kindLabel: string;
  kindColor: string;
  relativeTime: string;
  isLatest: boolean;
  /** 覆盖的卡底色(与外层 Pressable 背景一致)。SVG 渐隐遮罩/折角缺口需要与卡底色
   *  严格一致,而 SVG 不认 PlatformColor,故仅当传入字符串色值时生效。 */
  surfaceColor?: ClipboardCardProps['surfaceColor'];
}

// 传入 surfaceColor(字符串)时用它,否则按明暗回退 secondarySystemGroupedBackground 的实际色值
function resolveCardBg(surfaceColor: CardBodyProps['surfaceColor'], isDark: boolean): string {
  if (typeof surfaceColor === 'string') return surfaceColor;
  return isDark ? '#1C1C1E' : '#FFFFFF';
}

function CardBody(props: CardBodyProps) {
  switch (props.displayKind) {
    case 'image':
      return <ImageCardBody {...props} />;
    case 'url':
      return <URLCardBody {...props} />;
    case 'file':
    case 'group':
      return <FileCardBody {...props} />;
    default:
      return <TextCardBody {...props} />;
  }
}

function HeaderRow({
  kindLabel,
  relativeTime,
  overlay,
}: {
  kindLabel: string;
  relativeTime: string;
  overlay?: boolean;
}) {
  return (
    <View style={styles.headerRow}>
      <Text
        style={[
          styles.kindLabel,
          { color: overlay ? '#fff' : iosColors!.secondaryLabel },
          overlay && styles.kindLabelOverlay,
        ]}
      >
        {kindLabel}
      </Text>
      <Text
        style={[
          styles.timeLabel,
          { color: overlay ? 'rgba(255,255,255,0.8)' : iosColors!.tertiaryLabel },
        ]}
      >
        {relativeTime}
      </Text>
    </View>
  );
}

function BottomRow({
  isLatest,
  overlay,
  meta,
}: {
  isLatest: boolean;
  overlay?: boolean;
  meta?: string;
}) {
  return (
    <View style={styles.bottomRow}>
      {meta ? (
        <Text
          style={[styles.bottomMeta, { color: iosColors!.tertiaryLabel }]}
          numberOfLines={1}
          ellipsizeMode="tail"
        >
          {meta}
        </Text>
      ) : null}
      <View style={styles.bottomSpacer} />
      {isLatest && (
        <View style={[styles.latestDot, { backgroundColor: overlay ? '#fff' : iosAccentColor }]} />
      )}
    </View>
  );
}

// 让 SVG 由父级原生布局直接拉满。卡片在平板旋转时保持固定内容尺寸,只缩放外框,
// 因此这里无需为每个遮罩记录尺寸或触发额外 React 更新。
function SvgFill({ style, children }: { style?: StyleProp<ViewStyle>; children: React.ReactNode }) {
  return (
    <View style={style ?? StyleSheet.absoluteFill} pointerEvents="none">
      <Svg style={StyleSheet.absoluteFill}>{children}</Svg>
    </View>
  );
}

function GradientScrim({
  direction,
  opacity,
  style,
}: {
  direction: 'down' | 'up';
  opacity: number;
  style: any;
}) {
  const id = direction === 'down' ? 'scrimDown' : 'scrimUp';
  return (
    <SvgFill style={style}>
      <Defs>
        <SvgLinearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <Stop offset="0" stopColor="black" stopOpacity={direction === 'down' ? opacity : 0} />
          <Stop offset="1" stopColor="black" stopOpacity={direction === 'down' ? 0 : opacity} />
        </SvgLinearGradient>
      </Defs>
      <SvgRect width="100%" height="100%" fill={`url(#${id})`} />
    </SvgFill>
  );
}

const CHECKER_CELL = 16;

// 透明区域棋盘格背景。用单个 SVG Pattern 平铺，替代 12×12=144 个 RN View，
// 每张图片卡片只产生一个原生视图。
function CheckerboardBackground() {
  return (
    <SvgFill>
      <Defs>
        <SvgPattern
          id="checker"
          width={CHECKER_CELL * 2}
          height={CHECKER_CELL * 2}
          patternUnits="userSpaceOnUse"
        >
          <SvgRect
            width={CHECKER_CELL * 2}
            height={CHECKER_CELL * 2}
            fill="rgba(166,166,166,0.25)"
          />
          <SvgRect width={CHECKER_CELL} height={CHECKER_CELL} fill="rgba(217,217,217,0.25)" />
          <SvgRect
            transform={`translate(${CHECKER_CELL}, ${CHECKER_CELL})`}
            width={CHECKER_CELL}
            height={CHECKER_CELL}
            fill="rgba(217,217,217,0.25)"
          />
        </SvgPattern>
      </Defs>
      <SvgRect width="100%" height="100%" fill="url(#checker)" />
    </SvgFill>
  );
}

// 「引文排版」：短文本升大字号垂直居中像引言卡；长文本小字号排到底、
// 行尾用卡底色渐变遮罩渐隐，代替 numberOfLines 硬截断
const QUOTE_MAX_CHARS = 26;
// 超出可视区的文本对排版无贡献，截断以免超长剪贴内容拖慢布局
const PARA_RENDER_CHARS = 400;

function TextCardBody({ item, kindLabel, relativeTime, isLatest, surfaceColor }: CardBodyProps) {
  const { theme } = useTheme();
  const text = item.text.trim();
  const isQuote = text.length <= QUOTE_MAX_CHARS;
  // 渐隐遮罩必须与卡底色一致才能"隐入卡底"
  const fadeColor = resolveCardBg(surfaceColor, theme.isDark);

  return (
    <View style={styles.standardBody}>
      <HeaderRow kindLabel={kindLabel} relativeTime={relativeTime} />
      {isQuote ? (
        <View style={styles.quoteBody}>
          <Text style={[styles.quoteText, { color: iosColors!.label }]} numberOfLines={4}>
            {text}
          </Text>
        </View>
      ) : (
        <View style={styles.paraClip}>
          <Text style={[styles.paraText, { color: iosColors!.label }]}>
            {text.slice(0, PARA_RENDER_CHARS)}
          </Text>
          <TextFadeOut color={fadeColor} />
        </View>
      )}
      <BottomRow isLatest={isLatest} />
    </View>
  );
}

// 长文本底部的卡底色渐隐遮罩
function TextFadeOut({ color }: { color: string }) {
  return (
    <SvgFill style={styles.textFade}>
      <Defs>
        <SvgLinearGradient id="textFade" x1="0" y1="0" x2="0" y2="1">
          <Stop offset="0" stopColor={color} stopOpacity={0} />
          <Stop offset="1" stopColor={color} stopOpacity={1} />
        </SvgLinearGradient>
      </Defs>
      <SvgRect width="100%" height="100%" fill="url(#textFade)" />
    </SvgFill>
  );
}

// 「拟真文档页」：卡内一张带折角的纸，纸上是扩展名色块 + 文件名；
// 归档(group)在纸后叠两张旋转的"影子纸"表示多项
function FileCardBody({
  item,
  displayKind,
  kindLabel,
  relativeTime,
  isLatest,
  surfaceColor,
}: CardBodyProps) {
  const { t } = useTranslation('history');
  const { theme } = useTheme();
  const isGroup = displayKind === 'group';
  const fileName = item.dataName || item.text;
  const ext = getFileExtension(fileName);
  const chipLabel = isGroup ? t('kind.group') : ext || t('kind.file');
  const chipColor = isGroup ? iosKindTints.group : getExtensionColor(ext);
  const sizeLabel = item.size ? formatFileSize(item.size) : '';

  // 暗色下纸面要比卡底亮一阶;卡底被 surfaceColor 覆盖(tertiary #2C2C2E)时纸面/折角/影子纸
  // 顺势各上移一阶,否则纸面与卡底同色隐形
  const raised = theme.isDark && typeof surfaceColor === 'string';
  const paperBg = theme.isDark ? (raised ? '#3A3A3C' : '#2C2C2E') : '#FDFDFD';
  const paperBorder = theme.isDark ? 'rgba(255,255,255,0.10)' : 'rgba(0,0,0,0.08)';
  const foldColor = theme.isDark ? (raised ? '#48484A' : '#3A3A3C') : '#E3E3E8';
  const ghostBg = theme.isDark ? (raised ? '#2F2F31' : '#232325') : '#ECECF0';
  // 折角缺口要与卡底色一致才有"纸角被翻起"的效果
  const cardBg = resolveCardBg(surfaceColor, theme.isDark);

  return (
    <View style={styles.standardBody}>
      <HeaderRow kindLabel={kindLabel} relativeTime={relativeTime} />
      <View style={styles.paperWrap}>
        <View style={styles.paperStack}>
          {isGroup && (
            <>
              <View
                style={[styles.paperGhost, styles.paperGhostLeft, { backgroundColor: ghostBg }]}
              />
              <View
                style={[styles.paperGhost, styles.paperGhostRight, { backgroundColor: ghostBg }]}
              />
            </>
          )}
          <View style={[styles.paper, { backgroundColor: paperBg, borderColor: paperBorder }]}>
            <View style={[styles.paperCutout, { backgroundColor: cardBg }]}>
              <View style={[styles.paperFold, { borderLeftColor: foldColor }]} />
            </View>
            <View style={[styles.extChip, { backgroundColor: chipColor }]}>
              <Text style={styles.extChipText}>{chipLabel}</Text>
            </View>
            <Text
              style={[styles.paperName, { color: iosColors!.secondaryLabel }]}
              numberOfLines={2}
            >
              {stripExtension(fileName)}
            </Text>
          </View>
        </View>
      </View>
      <BottomRow isLatest={isLatest} meta={sizeLabel} />
    </View>
  );
}

function ImageCardBody({ item, kindColor, kindLabel, relativeTime, isLatest }: CardBodyProps) {
  const [loadFailed, setLoadFailed] = useState(false);
  const hasImage = item.isLocalFileReady && item.fileUri && !loadFailed;
  return (
    <View style={styles.imageBody}>
      <CheckerboardBackground />
      {hasImage ? (
        <Image
          source={{ uri: item.fileUri }}
          style={styles.thumbnailImage}
          resizeMode="contain"
          onError={() => setLoadFailed(true)}
        />
      ) : (
        <View style={[styles.imagePlaceholder, { backgroundColor: hexToRgba(kindColor, 0.12) }]}>
          <ImageIcon size={36} color={hexToRgba(kindColor, 0.5)} />
        </View>
      )}
      <GradientScrim direction="down" opacity={0.45} style={styles.topScrim} />
      <GradientScrim direction="up" opacity={0.35} style={styles.bottomScrim} />
      <View style={styles.imageOverlay}>
        <HeaderRow kindLabel={kindLabel} relativeTime={relativeTime} overlay />
        <View style={styles.spacer} />
        <BottomRow isLatest={isLatest} overlay />
      </View>
    </View>
  );
}

// 方案 A「沉浸全出血」：OG 图铺满整卡，底部 scrim 上叠 favicon + 域名 + 两行标题；
// 无 OG 图时用域名派生色渐变 + 居中 favicon 兜底
function URLCardBody({ item, kindLabel, relativeTime, isLatest }: CardBodyProps) {
  const metadata = useURLMetadata(item.text.trim());
  const domain = getURLDomain(item.text);
  const urlText = getURLWithoutScheme(item.text);
  const hasOgImage = !!metadata?.ogImageUrl;
  const displayTitle = metadata?.title || urlText;
  const gradient = getDomainGradient(domain);

  return (
    <View style={styles.urlBody}>
      <DomainGradientBackground gradient={gradient} />
      {metadata && !hasOgImage && (
        <View style={styles.urlFallbackCenter}>
          <Favicon url={metadata.faviconUrl} domain={domain} size={44} />
        </View>
      )}
      {hasOgImage && (
        <Image source={{ uri: metadata!.ogImageUrl }} style={styles.ogImage} resizeMode="cover" />
      )}
      <GradientScrim direction="down" opacity={0.5} style={styles.topScrim} />
      <URLBottomScrim />
      <View style={styles.urlOverlay}>
        <HeaderRow kindLabel={kindLabel} relativeTime={relativeTime} overlay />
        <View style={styles.spacer} />
        <View style={styles.urlDomainRow}>
          <Favicon url={metadata?.faviconUrl} domain={domain} size={16} />
          <Text style={styles.urlDomain} numberOfLines={1}>
            {domain}
          </Text>
          <View style={styles.bottomSpacer} />
          {isLatest && <View style={[styles.latestDot, styles.urlLatestDot]} />}
        </View>
        <Text style={styles.urlTitle} numberOfLines={2}>
          {displayTitle}
        </Text>
      </View>
    </View>
  );
}

// 无 OG 图时的整卡对角渐变兜底，颜色由域名哈希派生，同域名恒定
function DomainGradientBackground({ gradient }: { gradient: DomainGradient }) {
  return (
    <SvgFill>
      <Defs>
        <SvgLinearGradient id="domainBg" x1="0" y1="0" x2="1" y2="1">
          <Stop offset="0" stopColor={gradient.start} stopOpacity={1} />
          <Stop offset="1" stopColor={gradient.end} stopOpacity={1} />
        </SvgLinearGradient>
      </Defs>
      <SvgRect width="100%" height="100%" fill="url(#domainBg)" />
    </SvgFill>
  );
}

// 底部文字区 scrim：比图片卡的更高更深，保证 favicon/域名/标题在任意 OG 图上可读
function URLBottomScrim() {
  return (
    <SvgFill style={styles.urlBottomScrim}>
      <Defs>
        <SvgLinearGradient id="urlScrim" x1="0" y1="0" x2="0" y2="1">
          <Stop offset="0" stopColor="black" stopOpacity={0} />
          <Stop offset="0.55" stopColor="black" stopOpacity={0.5} />
          <Stop offset="1" stopColor="black" stopOpacity={0.78} />
        </SvgLinearGradient>
      </Defs>
      <SvgRect width="100%" height="100%" fill="url(#urlScrim)" />
    </SvgFill>
  );
}

// favicon 图；缺失或解码失败（如 Android 不支持的 .ico）时回退为白底域名字标
function Favicon({ url, domain, size }: { url?: string; domain: string; size: number }) {
  const [failed, setFailed] = useState(false);
  const shape = {
    width: size,
    height: size,
    borderRadius: Math.round(size * 0.28),
  };
  if (url && !failed) {
    return (
      <Image
        source={{ uri: url }}
        style={[styles.faviconTile, shape]}
        onError={() => setFailed(true)}
      />
    );
  }
  return (
    <View style={[styles.faviconTile, styles.faviconFallback, shape]}>
      <Text
        style={{
          color: getDomainGradient(domain).start,
          fontWeight: '800',
          fontSize: Math.round(size * 0.55),
        }}
      >
        {getDomainInitial(domain)}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  fill: {
    flex: 1,
  },
  card: {
    flex: 1,
    borderRadius: iosDimensions.cardCornerRadius,
    borderCurve: 'continuous' as any,
    overflow: 'hidden',
    // 阴影按需在组件内加(仅 compact 浮起态);凹陷态(surfaceColor)不加,见上方 recessed。
  },
  standardBody: {
    flex: 1,
    padding: 12,
  },
  quoteBody: {
    flex: 1,
    justifyContent: 'center',
  },
  quoteText: {
    fontSize: 17,
    lineHeight: 24,
    fontWeight: '600',
    letterSpacing: 0.2,
  },
  paraClip: {
    flex: 1,
    marginTop: 8,
    marginBottom: 6,
    overflow: 'hidden',
  },
  paraText: {
    fontSize: 13,
    lineHeight: 18.5,
  },
  textFade: {
    position: 'absolute',
    left: 0,
    right: 0,
    bottom: 0,
    height: 22,
  },
  paperWrap: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
  },
  paperStack: {
    width: 92,
    height: 112,
  },
  paperGhost: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    borderRadius: 6,
  },
  paperGhostLeft: {
    transform: [{ rotate: '-5deg' }, { translateX: -4 }, { translateY: 2 }],
  },
  paperGhostRight: {
    transform: [{ rotate: '3deg' }, { translateX: 4 }, { translateY: 1 }],
  },
  paper: {
    flex: 1,
    borderRadius: 6,
    borderWidth: StyleSheet.hairlineWidth,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    paddingHorizontal: 8,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 2 },
    shadowOpacity: 0.12,
    shadowRadius: 3,
  },
  paperCutout: {
    position: 'absolute',
    top: 0,
    right: 0,
    width: 18,
    height: 18,
  },
  paperFold: {
    width: 0,
    height: 0,
    borderLeftWidth: 18,
    borderTopWidth: 18,
    borderTopColor: 'transparent',
  },
  extChip: {
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderRadius: 6,
  },
  extChipText: {
    color: '#fff',
    fontSize: 11,
    fontWeight: '700',
    letterSpacing: 0.4,
  },
  paperName: {
    fontSize: 10.5,
    lineHeight: 14,
    textAlign: 'center',
  },
  bottomMeta: {
    fontSize: 11,
    flexShrink: 1,
  },
  headerRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  kindLabel: {
    fontSize: 11,
  },
  kindLabelOverlay: {
    fontWeight: '500',
  },
  timeLabel: {
    fontSize: 11,
  },
  bottomRow: {
    flexDirection: 'row',
    alignItems: 'center',
  },
  bottomSpacer: {
    flex: 1,
  },
  latestDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
  },
  spacer: {
    flex: 1,
  },
  imageBody: {
    flex: 1,
  },
  thumbnailImage: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    // 仅用四边 inset 拉满,不再叠 width/height:'100%' —— 绝对定位下百分比会覆盖 right/bottom
    // 并按父容器宽高的百分比独立取整,卡片宽为小数时比父容器少 1px,右/下露出底层 1px 浅色缝。
  },
  imagePlaceholder: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    alignItems: 'center',
    justifyContent: 'center',
  },
  topScrim: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    height: 36,
  },
  bottomScrim: {
    position: 'absolute',
    bottom: 0,
    left: 0,
    right: 0,
    height: 28,
  },
  imageOverlay: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    padding: 10,
    justifyContent: 'space-between',
  },
  urlBody: {
    flex: 1,
  },
  ogImage: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    // 仅用四边 inset 拉满,不再叠 width/height:'100%' —— 绝对定位下百分比会覆盖 right/bottom
    // 并按父容器宽高的百分比独立取整,卡片宽为小数时比父容器少 1px,右/下露出底层 1px 浅色缝。
  },
  urlFallbackCenter: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    alignItems: 'center',
    justifyContent: 'center',
  },
  urlBottomScrim: {
    position: 'absolute',
    left: 0,
    right: 0,
    bottom: 0,
    height: '62%',
  },
  urlOverlay: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    padding: 10,
    paddingBottom: 11,
  },
  urlDomainRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  urlDomain: {
    fontSize: 10,
    color: 'rgba(255,255,255,0.8)',
    flexShrink: 1,
  },
  urlTitle: {
    fontSize: 12.5,
    lineHeight: 17,
    fontWeight: '600',
    color: '#fff',
    marginTop: 5,
    textShadowColor: 'rgba(0,0,0,0.35)',
    textShadowOffset: { width: 0, height: 1 },
    textShadowRadius: 4,
  },
  urlLatestDot: {
    backgroundColor: '#fff',
  },
  faviconTile: {
    backgroundColor: 'rgba(255,255,255,0.92)',
  },
  faviconFallback: {
    alignItems: 'center',
    justifyContent: 'center',
  },
  selectIndicator: {
    position: 'absolute',
    right: 10,
    bottom: 10,
  },
  checkBadge: {
    width: 24,
    height: 24,
    borderRadius: 12,
    borderWidth: 2,
    borderColor: '#FFFFFF',
    alignItems: 'center',
    justifyContent: 'center',
  },
  uncheckedBadge: {
    width: 22,
    height: 22,
    borderRadius: 11,
    borderWidth: 1.5,
    borderColor: 'rgba(60,60,67,0.35)',
    backgroundColor: 'rgba(255,255,255,0.9)',
  },
  copiedBadge: {
    position: 'absolute',
    top: 8,
    right: 8,
    height: 24,
    paddingLeft: 7,
    paddingRight: 9,
    borderRadius: 12,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
  },
  copiedBadgeText: { fontSize: 12, fontWeight: '600' },
});
