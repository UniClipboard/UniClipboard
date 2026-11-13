import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  DynamicColorIOS,
  Keyboard,
  Modal,
  PlatformColor,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  useWindowDimensions,
  View,
  type LayoutChangeEvent,
  type ViewStyle,
} from 'react-native';
import Animated, { FadeIn, FadeOut } from 'react-native-reanimated';
import { Host, Picker, Text as SwiftUIText } from '@expo/ui/swift-ui';
import { pickerStyle, tag } from '@expo/ui/swift-ui/modifiers';
import { ChevronDown, ChevronLeft, ChevronUp, Copy, Globe, Share } from 'lucide-react-native';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import { GestureDetector, GestureHandlerRootView, ScrollView } from 'react-native-gesture-handler';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { ConnectedMessageToast } from '@/components/ConnectedMessageToast';
import { usePagePushTransition } from '@/hooks/usePagePushTransition';
import { useTheme } from '@/hooks/useTheme';
import { useWordPicker, type TokenFrame, type WordPickerOptions } from '@/hooks/useWordPicker';
import { useWordPickerHint } from '@/hooks/useWordPickerHint';
import { iosColors } from '@/theme/iosDesignTokens';

// RN Modal 独立窗口里 Liquid Glass 不生效:浮在内容上的控件与面板用实色 + 阴影
const FLOATING_FILL = PlatformColor('secondarySystemGroupedBackground');
const PANEL_FILL = DynamicColorIOS({ light: '#F9F9F9', dark: '#2C2C2E' });
import type { ColorScheme } from '@/theme/colors';
import type { CopyJoinMode, SegToken } from '@/utils/wordSegmentation';
import type { WordPickerOverlayProps } from './WordPickerOverlay.types';

/** 词块高度 */
const TILE_HEIGHT = 38;
/** 词块之间的水平间隔。它画在每个词块的前导区里，连成高亮带时填成强调色 */
const TILE_GAP = 6;
/** 拖柄圆点中心到词块上 / 下边缘的距离(系统文本选择样式:起点在上、终点在下) */
const HANDLE_OFFSET_Y = 10;
const TILE_RADIUS = 8;
const JOIN_MODES: CopyJoinMode[] = ['original', 'space', 'newline'];

const PICKER_OPTIONS: WordPickerOptions = {
  holdMode: 'range',
  handles: { offsetY: HANDLE_OFFSET_Y, hitRadius: 24, startEdge: 'top' },
};

/**
 * 分词选择页(iOS / Liquid Glass):从详情页以页面形式推入。
 * 玻璃顶栏(返回 / 标题 / 全选)、分词 / 逐字分段控件、首次手势提示、词块流,以及底部浮动
 * 玻璃面板(统计 + 两行预览 + 搜索 / 分享 / 复制);点预览或上拉展开为可编辑的复制前预览。
 * 交互与 Android 相同,全部在 useWordPicker,本文件只负责 iOS 皮肤。
 */
export function WordPickerOverlay({ text, deviceName, onDismiss }: WordPickerOverlayProps) {
  const { t } = useTranslation('history');
  const { theme } = useTheme();
  const colors = theme.colors;
  const insets = useSafeAreaInsets();

  // 转场需要 picker 的 beginTokenization，picker 需要转场的 close：用 ref 蹦床解开互相依赖
  const beginRef = useRef<() => void>(() => {});
  const page = usePagePushTransition(onDismiss, () => beginRef.current(), 1);
  const picker = useWordPicker(text, page.close, PICKER_OPTIONS);
  useLayoutEffect(() => {
    beginRef.current = picker.beginTokenization;
  }, [picker.beginTokenization]);

  const hasSelection = picker.selectedCount > 0;
  const showHint = useWordPickerHint(hasSelection);
  const [expanded, setExpanded] = useState(false);
  // 选区清空时收起预览（渲染期直接纠正，不绕一轮 effect）
  if (expanded && !hasSelection) setExpanded(false);

  const [panelHeight, setPanelHeight] = useState(0);
  const onPanelLayout = useCallback((e: LayoutChangeEvent) => {
    if (!e.nativeEvent.layout) return;
    setPanelHeight(Math.round(e.nativeEvent.layout.height));
  }, []);

  return (
    <Modal visible transparent animationType="none" onRequestClose={() => page.close()}>
      {/* RNGH 在 RN Modal 里需要自己的根，否则手势可能静默失效 */}
      <GestureHandlerRootView style={StyleSheet.absoluteFill}>
        <Animated.View
          style={[
            StyleSheet.absoluteFill,
            { backgroundColor: iosColors?.systemGroupedBackground, paddingTop: insets.top },
            page.pageStyle,
          ]}
        >
          <View style={s.header}>
            <Pressable
              testID="word-picker-back"
              onPress={() => page.close()}
              accessibilityRole="button"
              accessibilityLabel={t('action.back', { ns: 'common' })}
            >
              <View style={[s.circleButton, s.floatingSurface]}>
                <ChevronLeft size={24} color={colors.textPrimary} />
              </View>
            </Pressable>
            <Text style={[s.title, { color: colors.textPrimary }]} numberOfLines={1} accessibilityRole="header">
              {t('menu.selectText')}
            </Text>
            <Pressable
              testID="word-picker-select-all"
              onPress={picker.toggleSelectAll}
              disabled={!picker.hasSelectableTokens}
              accessibilityRole="button"
              accessibilityState={{ disabled: !picker.hasSelectableTokens }}
            >
              <View style={[s.textPill, s.floatingSurface]}>
                <Text
                  style={[
                    s.textPillLabel,
                    { color: picker.hasSelectableTokens ? colors.textPrimary : colors.textDisabled },
                  ]}
                  numberOfLines={1}
                >
                  {picker.allSelected ? t('wordPicker.deselectAll') : t('action.selectAll', { ns: 'common' })}
                </Text>
              </View>
            </Pressable>
          </View>

          <View style={s.modeRow}>
            <Host style={s.segmented}>
              <Picker
                testID="word-picker-granularity"
                selection={picker.granularity}
                onSelectionChange={(value) => picker.setGranularity(value === 'char' ? 'char' : 'word')}
                modifiers={[pickerStyle('segmented')]}
              >
                <SwiftUIText modifiers={[tag('word')]}>{t('wordPicker.granularity.word')}</SwiftUIText>
                <SwiftUIText modifiers={[tag('char')]}>{t('wordPicker.granularity.char')}</SwiftUIText>
              </Picker>
            </Host>
            {deviceName ? (
              <Text style={[s.deviceLabel, { color: colors.textSecondary }]} numberOfLines={1}>
                {t('detail.fromDevice', { ns: 'home', device: deviceName })}
              </Text>
            ) : null}
          </View>

          {showHint ? (
            <Text testID="word-picker-hints" style={[s.hint, { color: colors.textSecondary }]}>
              {(['tap', 'swipe', 'hold'] as const).map((key) => t(`wordPicker.hints.${key}`)).join(' · ')}
            </Text>
          ) : null}

          {picker.truncated ? (
            <Text style={[s.hint, { color: colors.textSecondary }]}>
              {t('wordPicker.truncated', { count: 5000 })}
            </Text>
          ) : null}

          {picker.status === 'preparing' ? (
            <SkeletonRows />
          ) : picker.hasSelectableTokens ? (
            <ScrollView
              style={s.scroll}
              contentContainerStyle={[s.scrollContent, { paddingBottom: panelHeight + 32 }]}
            >
              <GestureDetector gesture={picker.paintGesture}>
                <View style={s.flow} collapsable={false}>
                  {picker.tokens.map((token, index) =>
                    // 普通空格不占位（间距由前导区给出），含换行的空白强制断行
                    token.isWhitespace ? (
                      token.text.includes('\n') ? (
                        <LineBreak key={token.start} text={token.text} />
                      ) : null
                    ) : (
                      <FlowItem
                        key={token.start}
                        index={index}
                        token={token}
                        charMode={picker.granularity === 'char'}
                        isSelected={picker.selected.has(index)}
                        inBand={picker.band[index] != null}
                        joinPrev={picker.band[index]?.joinPrev ?? false}
                        joinNext={picker.band[index]?.joinNext ?? false}
                        startHandle={picker.handleRun?.firstSelected === index}
                        endHandle={picker.handleRun?.lastSelected === index}
                        onPress={picker.toggleToken}
                        onFrame={picker.registerTokenLayout}
                        colors={colors}
                      />
                    )
                  )}
                </View>
              </GestureDetector>
            </ScrollView>
          ) : (
            <View style={s.emptyWrap}>
              <Text style={[s.emptyText, { color: colors.textSecondary }]}>{t('wordPicker.empty')}</Text>
            </View>
          )}

          {expanded ? (
            <Animated.View
              entering={FadeIn.duration(200)}
              exiting={FadeOut.duration(150)}
              style={[StyleSheet.absoluteFill, s.scrim]}
            >
              <Pressable
                testID="word-picker-scrim"
                style={StyleSheet.absoluteFill}
                onPress={() => setExpanded(false)}
                accessibilityLabel={t('wordPicker.collapsePreview')}
              />
            </Animated.View>
          ) : null}

          <SelectionPanel
            picker={picker}
            expanded={expanded}
            onExpandedChange={setExpanded}
            onLayout={onPanelLayout}
            bottomInset={insets.bottom}
            colors={colors}
          />
        </Animated.View>

        {/* 页面开着时发出的提示(如复制成功)要压在 Modal 内容之上才可见,并抬到面板之上 */}
        <ConnectedMessageToast bottomOffset={panelHeight + 16} />
      </GestureHandlerRootView>
    </Modal>
  );
}

type WordPicker = ReturnType<typeof useWordPicker>;

/**
 * 底部浮动玻璃面板:收起时是统计 + 两行预览,展开到大尺寸时是可编辑的复制前预览与拼接方式。
 * 预览可点按展开,抓手可上拉展开 / 下拉收起。没有选区时操作不可用。
 */
function SelectionPanel({
  picker,
  expanded,
  onExpandedChange,
  onLayout,
  bottomInset,
  colors,
}: {
  picker: WordPicker;
  expanded: boolean;
  onExpandedChange: (expanded: boolean) => void;
  onLayout: (e: LayoutChangeEvent) => void;
  bottomInset: number;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('history');
  const { height: screenH } = useWindowDimensions();
  const keyboardHeight = useKeyboardHeight();
  const expandedHeight = Math.min(520, Math.round(screenH * 0.6));
  // 键盘弹出时面板贴在键盘之上
  const bottom = keyboardHeight > 0 ? keyboardHeight + 8 : 8;
  const paddingBottom = keyboardHeight > 0 ? 16 : Math.max(16, bottomInset - 12);

  return (
    <View
      testID="word-picker-tray"
      onLayout={onLayout}
      style={[s.panelSlot, { bottom }, expanded && { height: expandedHeight }]}
    >
      <View style={[s.panel, s.panelSurface, { paddingBottom }]}>
        <Pressable
          testID="word-picker-grip"
          onPress={() => picker.selectedCount > 0 && onExpandedChange(!expanded)}
          accessibilityRole="button"
          accessibilityLabel={t(expanded ? 'wordPicker.collapsePreview' : 'wordPicker.expandPreview')}
          hitSlop={{ top: 8, bottom: 8, left: 40, right: 40 }}
          style={s.gripSlot}
        >
          <View style={[s.grip, { backgroundColor: iosColors?.tertiaryLabel }]} />
        </Pressable>
        {expanded ? (
          <ExpandedPanelContent picker={picker} onCollapse={() => onExpandedChange(false)} colors={colors} />
        ) : (
          <CollapsedPanelContent picker={picker} onExpand={() => onExpandedChange(true)} colors={colors} />
        )}
        <PanelActions picker={picker} showSearch={!expanded} disabled={picker.selectedCount === 0} colors={colors} />
      </View>
    </View>
  );
}

/** 收起态：选区统计 + 清除，以及两行截断的预览（点按展开） */
function CollapsedPanelContent({
  picker,
  onExpand,
  colors,
}: {
  picker: WordPicker;
  onExpand: () => void;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('history');
  const hasSelection = picker.selectedCount > 0;
  const foreground = hasSelection ? colors.textPrimary : colors.textSecondary;
  return (
    <>
      <View style={s.summaryRow}>
        <Text testID="word-picker-summary" style={[s.summary, { color: foreground }]} numberOfLines={1}>
          {selectionSummary(t, picker)}
        </Text>
        {hasSelection ? (
          <Pressable
            testID="word-picker-clear"
            onPress={picker.clearSelection}
            accessibilityRole="button"
            hitSlop={{ top: 8, bottom: 8 }}
          >
            <Text style={s.link}>{t('action.clear', { ns: 'common' })}</Text>
          </Pressable>
        ) : null}
      </View>
      <Pressable
        testID="word-picker-preview"
        onPress={onExpand}
        disabled={!hasSelection}
        accessibilityRole="button"
        accessibilityLabel={t('wordPicker.expandPreview')}
        accessibilityState={{ disabled: !hasSelection }}
        style={[s.preview, { backgroundColor: iosColors?.secondarySystemGroupedBackground }]}
      >
        <Text style={[s.previewText, { color: foreground }]} numberOfLines={2} ellipsizeMode="tail">
          {hasSelection ? picker.outputText : t('wordPicker.placeholder')}
        </Text>
        {hasSelection ? <ChevronUp size={20} color={colors.textTertiary} /> : null}
      </Pressable>
    </>
  );
}

/** 展开态：可直接编辑的复制前预览；选区有多个连续段时给出拼接方式 */
function ExpandedPanelContent({
  picker,
  onCollapse,
  colors,
}: {
  picker: WordPicker;
  onCollapse: () => void;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('history');
  return (
    <>
      <View style={s.expandedHeader}>
        <Text style={[s.expandedTitle, { color: colors.textPrimary }]}>{t('wordPicker.previewTitle')}</Text>
        <Pressable
          testID="word-picker-collapse"
          onPress={onCollapse}
          accessibilityRole="button"
          accessibilityLabel={t('wordPicker.collapsePreview')}
          hitSlop={6}
          style={[s.closeCircle, { backgroundColor: iosColors?.tertiarySystemFill }]}
        >
          <ChevronDown size={18} color={colors.textSecondary} />
        </Pressable>
      </View>
      <View style={s.fieldBlock}>
        <Text style={[s.fieldLabel, { color: colors.textSecondary }]}>{t('wordPicker.editable')}</Text>
        <TextInput
          testID="word-picker-editor"
          value={picker.outputText}
          onChangeText={picker.setOutputText}
          multiline
          textAlignVertical="top"
          selectionColor={colors.accent}
          style={[
            s.editor,
            { backgroundColor: iosColors?.secondarySystemGroupedBackground, color: colors.textPrimary },
          ]}
        />
      </View>
      {picker.runs.length > 1 ? (
        <View style={s.fieldBlock}>
          <Text style={[s.fieldLabel, { color: colors.textSecondary }]}>{t('wordPicker.joinLabel')}</Text>
          <Host style={s.joinPicker}>
            <Picker
              testID="word-picker-join"
              selection={picker.joinMode}
              onSelectionChange={(value) => picker.setJoinMode(value as CopyJoinMode)}
              modifiers={[pickerStyle('segmented')]}
            >
              {JOIN_MODES.map((mode) => (
                <SwiftUIText key={mode} modifiers={[tag(mode)]}>
                  {t(`wordPicker.join.${mode}`)}
                </SwiftUIText>
              ))}
            </Picker>
          </Host>
        </View>
      ) : null}
      <View style={s.fill} />
    </>
  );
}

/** 「已选 N 个词 · M 字」；逐字粒度只报字数，没有选区时为「未选择」 */
function selectionSummary(t: TFunction<'history'>, picker: WordPicker): string {
  if (picker.selectedCount === 0) return t('wordPicker.noneSelected');
  const chars = t('wordPicker.summary.chars', { count: picker.charCount });
  if (picker.granularity === 'char') {
    return t('wordPicker.summary.charsOnly', { count: picker.charCount });
  }
  return `${t('wordPicker.summary.words', { count: picker.selectedCount })} · ${chars}`;
}

/** 面板底部操作行:网页搜索(收起态)、分享与复制;两种形态共用,入口一致 */
function PanelActions({
  picker,
  showSearch,
  disabled,
  colors,
}: {
  picker: WordPicker;
  showSearch: boolean;
  disabled: boolean;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('history');
  const iconColor = disabled ? colors.textDisabled : colors.textPrimary;
  return (
    <View style={s.actionRow}>
      {showSearch ? (
        <Pressable
          testID="word-picker-search"
          onPress={picker.searchSelected}
          disabled={disabled}
          accessibilityRole="button"
          accessibilityLabel={t('wordPicker.webSearch')}
          accessibilityState={{ disabled }}
          style={[s.roundAction, { backgroundColor: iosColors?.tertiarySystemFill }]}
        >
          <Globe size={22} color={iconColor} />
        </Pressable>
      ) : null}
      <Pressable
        testID="word-picker-share"
        onPress={picker.shareSelected}
        disabled={disabled}
        accessibilityRole="button"
        accessibilityLabel={t('action.share', { ns: 'common' })}
        accessibilityState={{ disabled }}
        style={[s.roundAction, { backgroundColor: iosColors?.tertiarySystemFill }]}
      >
        <Share size={22} color={iconColor} />
      </Pressable>
      <Pressable
        testID="word-picker-copy"
        onPress={picker.copySelected}
        disabled={disabled}
        accessibilityRole="button"
        accessibilityState={{ disabled }}
        style={[s.copyButton, { backgroundColor: disabled ? iosColors?.tertiarySystemFill : colors.accent }]}
      >
        <Copy size={20} color={disabled ? colors.textDisabled : colors.inverseAccent} />
        <Text style={[s.copyLabel, { color: disabled ? colors.textDisabled : colors.inverseAccent }]}>
          {t('action.copy', { ns: 'common' })}
        </Text>
      </Pressable>
    </View>
  );
}

/** Modal 不随软键盘缩放:面板自己抬到键盘之上 */
function useKeyboardHeight(): number {
  const [height, setHeight] = useState(0);
  useEffect(() => {
    const show = Keyboard.addListener('keyboardWillShow', (e) => setHeight(e.endCoordinates.height));
    const hide = Keyboard.addListener('keyboardWillHide', () => setHeight(0));
    return () => {
      show.remove();
      hide.remove();
    };
  }, []);
  return height;
}

function SkeletonRows() {
  return (
    <View style={s.skeletonWrap}>
      {(['92%', '78%', '86%', '60%'] as const).map((width) => (
        <View key={width} style={[s.skeletonBar, { width, backgroundColor: iosColors?.tertiarySystemFill }]} />
      ))}
    </View>
  );
}

interface FlowItemProps {
  index: number;
  token: SegToken;
  charMode: boolean;
  isSelected: boolean;
  inBand: boolean;
  joinPrev: boolean;
  joinNext: boolean;
  startHandle: boolean;
  endHandle: boolean;
  onPress: (index: number) => void;
  onFrame: (index: number, frame: TokenFrame) => void;
  colors: ColorScheme;
}

/**
 * 词流中的一项：词块或标点（空白由外层处理）。每项左侧带一个 TILE_GAP 宽的前导区作为间隔：
 * 与前一项连成高亮带时填成强调色，并画一条淡分隔线，相邻选中词读起来是一整段。
 * 布局固定，不随选区变化，涂选过程中命中测试的坐标不会漂移。
 */
const FlowItem = React.memo(function FlowItem({
  index,
  token,
  charMode,
  isSelected,
  inBand,
  joinPrev,
  joinNext,
  startHandle,
  endHandle,
  onPress,
  onFrame,
  colors,
}: FlowItemProps) {
  const { t } = useTranslation('history');
  const [lineStart, setLineStart] = useState(false);

  const onLayout = (e: LayoutChangeEvent) => {
    const { x, y, width, height } = e.nativeEvent.layout;
    setLineStart(x < 1);
    // 注册的是去掉前导区的词块本体，拖柄与命中都以本体为准
    onFrame(index, { x: x + TILE_GAP, y, width: width - TILE_GAP, height });
  };

  const joined = inBand && joinPrev && !lineStart;
  const rightRadius = inBand && joinNext ? 0 : TILE_RADIUS;
  const corners = {
    borderTopLeftRadius: joined ? 0 : TILE_RADIUS,
    borderBottomLeftRadius: joined ? 0 : TILE_RADIUS,
    borderTopRightRadius: rightRadius,
    borderBottomRightRadius: rightRadius,
  };

  return (
    <View
      onLayout={onLayout}
      style={[
        s.item,
        joined && [
          s.itemJoined,
          {
            backgroundColor: colors.accent,
            borderTopRightRadius: rightRadius,
            borderBottomRightRadius: rightRadius,
          },
        ],
        (startHandle || endHandle) && s.itemRaised,
      ]}
      collapsable={false}
    >
      <View style={s.lead}>
        {joined && !token.isPunctuation ? <View style={s.divider} /> : null}
      </View>
      {token.isPunctuation ? (
        <PunctuationMark text={token.text} inBand={inBand} corners={corners} colors={colors} />
      ) : (
        <TokenTile
          index={index}
          text={token.text}
          charMode={charMode}
          isSelected={isSelected}
          inBand={inBand}
          corners={corners}
          onPress={onPress}
          colors={colors}
        />
      )}
      {startHandle ? <SelectionHandle side="start" label={t('wordPicker.handleStart')} colors={colors} /> : null}
      {endHandle ? <SelectionHandle side="end" label={t('wordPicker.handleEnd')} colors={colors} /> : null}
    </View>
  );
});

type Corners = Pick<
  ViewStyle,
  'borderTopLeftRadius' | 'borderBottomLeftRadius' | 'borderTopRightRadius' | 'borderBottomRightRadius'
>;

/** 可点按的词块本体：在高亮带内填强调色 */
function TokenTile({
  index,
  text,
  charMode,
  isSelected,
  inBand,
  corners,
  onPress,
  colors,
}: {
  index: number;
  text: string;
  charMode: boolean;
  isSelected: boolean;
  inBand: boolean;
  corners: Corners;
  onPress: (index: number) => void;
  colors: ColorScheme;
}) {
  return (
    <Pressable
      testID={`word-picker-token-${index}`}
      onPress={() => onPress(index)}
      accessibilityRole="button"
      accessibilityState={{ selected: isSelected }}
      style={({ pressed }) => [
        s.tile,
        charMode && s.tileChar,
        corners,
        { backgroundColor: inBand ? colors.accent : iosColors?.tertiarySystemFill },
        pressed && s.tilePressed,
      ]}
    >
      <Text
        style={[s.tileText, { color: inBand ? colors.inverseAccent : colors.textPrimary }]}
        numberOfLines={1}
        ellipsizeMode="middle"
      >
        {text}
      </Text>
    </Pressable>
  );
}

/** 分词粒度下的标点：不可点，只作淡色分隔；夹在选中段内时随高亮带着色 */
function PunctuationMark({
  text,
  inBand,
  corners,
  colors,
}: {
  text: string;
  inBand: boolean;
  corners: Corners;
  colors: ColorScheme;
}) {
  return (
    <View style={[s.punct, corners, inBand && { backgroundColor: colors.accent }]}>
      <Text style={[s.tileText, { color: inBand ? colors.inverseAccent : colors.textSecondary }]}>{text}</Text>
    </View>
  );
}

/** 含换行的空白：强制断行；连续空行按行数加一点额外间距（最多两行） */
function LineBreak({ text }: { text: string }) {
  const extraLines = text.split('\n').length - 2;
  return <View style={[s.lineBreak, { height: Math.min(extraLines, 2) * 12 }]} />;
}

/**
 * 选区首 / 尾拖柄(纯展示;拖动由 useWordPicker 的手势按同一几何命中):系统文本选择样式,
 * 竖线贴着选区边缘,起点圆点在上方、终点圆点在下方。
 */
function SelectionHandle({
  side,
  label,
  colors,
}: {
  side: 'start' | 'end';
  label: string;
  colors: ColorScheme;
}) {
  const start = side === 'start';
  return (
    <View
      testID={`word-picker-handle-${side}`}
      accessibilityLabel={label}
      pointerEvents="none"
      style={[s.handle, start ? s.handleStart : s.handleEnd]}
    >
      {start ? <View style={[s.handleKnob, { backgroundColor: colors.accent, borderColor: iosColors?.systemGroupedBackground }]} /> : null}
      <View style={[s.handleStem, { backgroundColor: colors.accent }]} />
      {start ? null : <View style={[s.handleKnob, { backgroundColor: colors.accent, borderColor: iosColors?.systemGroupedBackground }]} />}
    </View>
  );
}

const HANDLE_KNOB = 12;
const HANDLE_STEM = TILE_HEIGHT + 6;

const s = StyleSheet.create({
  header: {
    height: 44,
    paddingHorizontal: 16,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  circleButton: { width: 44, height: 44, borderRadius: 22, alignItems: 'center', justifyContent: 'center' },
  floatingSurface: {
    backgroundColor: FLOATING_FILL,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 6 },
    shadowOpacity: 0.08,
    shadowRadius: 10,
  },
  panelSurface: {
    borderRadius: 36,
    borderCurve: 'continuous',
    backgroundColor: PANEL_FILL,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: -2 },
    shadowOpacity: 0.1,
    shadowRadius: 12,
  },
  title: { flex: 1, textAlign: 'center', fontSize: 17, fontWeight: '600' },
  textPill: { height: 44, paddingHorizontal: 16, borderRadius: 22, justifyContent: 'center' },
  textPillLabel: { fontSize: 16, fontWeight: '600' },
  modeRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    paddingHorizontal: 16,
    paddingTop: 12,
    paddingBottom: 12,
  },
  segmented: { width: 168, height: 32 },
  deviceLabel: { flexShrink: 1, fontSize: 13 },
  hint: { paddingHorizontal: 20, paddingBottom: 12, fontSize: 13 },
  scroll: { flex: 1 },
  scrollContent: { paddingTop: 12, paddingHorizontal: 16 },
  flow: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    rowGap: 10,
    // 抵消每项的前导区，使行首词块与页边距对齐
    marginLeft: -TILE_GAP,
  },
  item: { flexDirection: 'row', maxWidth: '80%' },
  // 连成带时整项（前导区 + 本体）铺带色，并向左多盖 1px，盖住取整留下的亚像素缝
  itemJoined: { marginLeft: -1, paddingLeft: 1 },
  itemRaised: { zIndex: 1 },
  lead: { width: TILE_GAP, height: TILE_HEIGHT, alignItems: 'center', justifyContent: 'center' },
  divider: { width: 1, height: 18, backgroundColor: 'rgba(255,255,255,0.25)' },
  lineBreak: { width: '100%' },
  tile: { height: TILE_HEIGHT, paddingHorizontal: 9, justifyContent: 'center', flexShrink: 1 },
  tileChar: { minWidth: TILE_HEIGHT, alignItems: 'center' },
  tilePressed: { opacity: 0.7 },
  tileText: { fontSize: 17, lineHeight: 22 },
  punct: { height: TILE_HEIGHT, minWidth: 12, alignItems: 'center', justifyContent: 'center' },
  handle: { position: 'absolute', width: HANDLE_KNOB, alignItems: 'center' },
  // 竖线中线对齐本体的左 / 右边缘;圆点圆心在词块上 / 下边缘外 HANDLE_OFFSET_Y 处
  handleStart: { left: TILE_GAP - HANDLE_KNOB / 2, top: -(HANDLE_OFFSET_Y + HANDLE_KNOB / 2) },
  handleEnd: { right: -HANDLE_KNOB / 2, top: 0 },
  handleStem: { width: 2, height: HANDLE_STEM - 2 },
  handleKnob: { width: HANDLE_KNOB, height: HANDLE_KNOB, borderRadius: HANDLE_KNOB / 2, borderWidth: 2 },
  emptyWrap: { flex: 1, alignItems: 'center', justifyContent: 'center' },
  emptyText: { fontSize: 17 },
  scrim: { backgroundColor: 'rgba(0,0,0,0.12)' },
  panelSlot: { position: 'absolute', left: 8, right: 8 },
  panel: { flexGrow: 1, paddingHorizontal: 16, paddingTop: 8, gap: 12 },
  gripSlot: { alignSelf: 'center', paddingVertical: 2 },
  grip: { width: 36, height: 5, borderRadius: 3 },
  summaryRow: { height: 30, flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
  summary: { flex: 1, fontSize: 15, fontWeight: '600' },
  link: { fontSize: 16, color: '#0063CC' },
  preview: {
    minHeight: 50,
    borderRadius: 16,
    borderCurve: 'continuous',
    paddingVertical: 12,
    paddingLeft: 14,
    paddingRight: 12,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  previewText: { flex: 1, fontSize: 16, lineHeight: 22 },
  expandedHeader: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
  expandedTitle: { fontSize: 20, fontWeight: '700' },
  closeCircle: { width: 32, height: 32, borderRadius: 16, alignItems: 'center', justifyContent: 'center' },
  fieldBlock: { gap: 6 },
  fieldLabel: { fontSize: 13, fontWeight: '600', paddingHorizontal: 4 },
  editor: {
    height: 128,
    borderRadius: 16,
    borderCurve: 'continuous',
    paddingHorizontal: 14,
    paddingTop: 12,
    paddingBottom: 12,
    fontSize: 17,
    lineHeight: 24,
  },
  joinPicker: { height: 32, alignSelf: 'stretch' },
  fill: { flex: 1 },
  actionRow: { flexDirection: 'row', alignItems: 'center', gap: 10 },
  roundAction: { width: 50, height: 50, borderRadius: 25, alignItems: 'center', justifyContent: 'center' },
  copyButton: {
    flex: 1,
    height: 50,
    borderRadius: 25,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
  },
  copyLabel: { fontSize: 17, fontWeight: '600' },
  skeletonWrap: { paddingHorizontal: 16, paddingTop: 12, gap: 14 },
  skeletonBar: { height: 16, borderRadius: 8 },
});
