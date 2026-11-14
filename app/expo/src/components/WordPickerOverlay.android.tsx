import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  Dimensions,
  Keyboard,
  Modal,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  useWindowDimensions,
  View,
  type LayoutChangeEvent,
  type ViewStyle,
} from 'react-native';
import Animated, { FadeIn, FadeOut, LinearTransition } from 'react-native-reanimated';
import Ionicons from '@expo/vector-icons/Ionicons';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import { GestureDetector, GestureHandlerRootView, ScrollView } from 'react-native-gesture-handler';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { ConnectedMessageToast } from '@/components/ConnectedMessageToast';
import { FilterChip } from '@/components/android/FilterChip';
import { M3IconButton } from '@/components/android/M3IconButton';
import { useTheme } from '@/hooks/useTheme';
import { usePagePushTransition } from '@/hooks/usePagePushTransition';
import { useWordPicker, type TokenFrame, type WordPickerOptions } from '@/hooks/useWordPicker';
import { useWordPickerHint } from '@/hooks/useWordPickerHint';
import { alpha, type ColorScheme } from '@/theme/colors';
import { m3Type } from '@/theme/m3Typography';
import type { CopyJoinMode, SegToken } from '@/utils/wordSegmentation';
import type { WordPickerOverlayProps } from './WordPickerOverlay.types';

/** 词块高度 */
const TILE_HEIGHT = 40;
/** 词块之间的水平间隔。它画在每个词块的前导区里，连成高亮带时填成强调色 */
const TILE_GAP = 6;
/** 拖柄圆点中心在词块底边之下的距离 */
const HANDLE_OFFSET_Y = 14;
const JOIN_MODES: CopyJoinMode[] = ['original', 'space', 'newline'];

const PICKER_OPTIONS: WordPickerOptions = {
  holdMode: 'range',
  handles: { offsetY: HANDLE_OFFSET_Y, hitRadius: 24 },
};

/**
 * 分词选择页（Android / M3）：从详情页以页面形式推入。
 * 顶栏（返回 / 标题 / 全选）、分词 / 逐字分段按钮、首次手势提示、词块流，
 * 以及底部托盘（统计 + 两行预览 + 搜索 / 分享 / 复制）；点预览展开为可编辑的复制前预览。
 * 交互全部在 useWordPicker，本文件只负责 M3 皮肤。
 */
export function WordPickerOverlay({
  text,
  deviceName,
  onSendTo,
  onDismiss,
}: WordPickerOverlayProps) {
  const { t } = useTranslation('history');
  const { theme } = useTheme();
  const colors = theme.colors;
  const insets = useSafeAreaInsets();

  // 转场需要 picker 的 beginTokenization，picker 需要转场的 close：用 ref 蹦床解开互相依赖
  const beginRef = useRef<() => void>(() => {});
  const page = usePagePushTransition(onDismiss, () => beginRef.current());
  const picker = useWordPicker(text, page.close, PICKER_OPTIONS);
  useLayoutEffect(() => {
    beginRef.current = picker.beginTokenization;
  }, [picker.beginTokenization]);

  const hasSelection = picker.selectedCount > 0;
  const showHint = useWordPickerHint(hasSelection);
  const [expanded, setExpanded] = useState(false);
  // 选区清空时收起预览（渲染期直接纠正，不绕一轮 effect）
  if (expanded && !hasSelection) setExpanded(false);

  const [trayHeight, setTrayHeight] = useState(0);
  const onTrayLayout = useCallback((e: LayoutChangeEvent) => {
    if (!e.nativeEvent.layout) return;
    setTrayHeight(Math.round(e.nativeEvent.layout.height));
  }, []);

  const onRequestClose = () => {
    if (expanded) setExpanded(false);
    else page.close();
  };

  return (
    <Modal
      visible
      transparent
      animationType="none"
      statusBarTranslucent
      navigationBarTranslucent
      onRequestClose={onRequestClose}
    >
      {/* RNGH 在 RN Modal 里需要自己的根，否则 Android 上手势静默失效 */}
      <GestureHandlerRootView style={StyleSheet.absoluteFill}>
        <Animated.View
          style={[
            StyleSheet.absoluteFill,
            { backgroundColor: colors.background, paddingTop: insets.top },
            page.pageStyle,
          ]}
        >
          <View style={s.appBar}>
            <M3IconButton
              testID="word-picker-back"
              icon="arrow-back"
              accessibilityLabel={t('action.back', { ns: 'common' })}
              onPress={() => page.close()}
              colors={colors}
              iconColor={colors.textPrimary}
            />
            <Text
              style={[s.appBarTitle, { color: colors.textPrimary }]}
              numberOfLines={1}
              accessibilityRole="header"
            >
              {t('menu.selectText')}
            </Text>
            <TextButton
              testID="word-picker-select-all"
              label={
                picker.allSelected
                  ? t('wordPicker.deselectAll')
                  : t('action.selectAll', { ns: 'common' })
              }
              onPress={picker.toggleSelectAll}
              disabled={!picker.hasSelectableTokens}
              colors={colors}
            />
          </View>

          <View style={s.modeRow}>
            <GranularityToggle
              value={picker.granularity}
              onChange={picker.setGranularity}
              colors={colors}
            />
            {deviceName ? (
              <Text
                style={[m3Type.bodySmall, s.deviceLabel, { color: colors.textSecondary }]}
                numberOfLines={1}
              >
                {t('detail.fromDevice', { ns: 'home', device: deviceName })}
              </Text>
            ) : null}
          </View>

          {showHint ? (
            <View style={s.hintRow} testID="word-picker-hints">
              {(['tap', 'swipe', 'hold'] as const).map((key) => (
                <View key={key} style={[s.hintChip, { backgroundColor: colors.surfaceLow }]}>
                  <Text style={[m3Type.bodySmall, { color: colors.textSecondary }]}>
                    {t(`wordPicker.hints.${key}`)}
                  </Text>
                </View>
              ))}
            </View>
          ) : null}

          {picker.truncated ? (
            <Text style={[m3Type.bodySmall, s.banner, { color: colors.textSecondary }]}>
              {t('wordPicker.truncated', { count: 5000 })}
            </Text>
          ) : null}

          {picker.status === 'preparing' ? (
            <SkeletonRows colors={colors} />
          ) : picker.hasSelectableTokens ? (
            <ScrollView
              style={s.scroll}
              contentContainerStyle={[s.scrollContent, { paddingBottom: trayHeight + 24 }]}
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
              <Text style={[m3Type.bodyLarge, { color: colors.textSecondary }]}>
                {t('wordPicker.empty')}
              </Text>
            </View>
          )}

          {expanded ? (
            <Animated.View
              entering={FadeIn.duration(200)}
              exiting={FadeOut.duration(150)}
              style={[
                StyleSheet.absoluteFill,
                { backgroundColor: alpha(colors.textPrimary, 0.32) },
              ]}
            >
              <Pressable
                testID="word-picker-scrim"
                style={StyleSheet.absoluteFill}
                onPress={() => setExpanded(false)}
                accessibilityLabel={t('wordPicker.collapsePreview')}
              />
            </Animated.View>
          ) : null}

          <SelectionTray
            picker={picker}
            expanded={expanded}
            onExpandedChange={setExpanded}
            animateLayout={page.settled}
            onSendTo={onSendTo}
            onLayout={onTrayLayout}
            bottomInset={insets.bottom}
            colors={colors}
          />
        </Animated.View>

        {/* 页面开着时发出的 snackbar（如复制失败）要压在 Modal 内容之上才可见，并抬到托盘之上 */}
        <ConnectedMessageToast bottomOffset={trayHeight + 8} />
      </GestureHandlerRootView>
    </Modal>
  );
}

type Picker = ReturnType<typeof useWordPicker>;

interface SelectionTrayProps {
  picker: Picker;
  expanded: boolean;
  onExpandedChange: (expanded: boolean) => void;
  onSendTo?: (text: string) => void;
  onLayout: (e: LayoutChangeEvent) => void;
  bottomInset: number;
  /**
   * 是否以过渡动画跟随布局变化。Modal 首次显示时根视图先按去掉系统栏的高度排版、随后才撑满，
   * 入场期间托盘会被这次修正挪动；冷启动时线程繁忙，这段布局过渡可能停在半途、托盘悬在高处。
   * 所以只在页面入场结束后才启用，用于展开 / 收起预览。
   */
  animateLayout: boolean;
  colors: ColorScheme;
}

/**
 * 底部托盘（M3 bottom sheet 外观）：收起时是统计 + 两行预览，展开时是可编辑的复制前预览
 * 与拼接方式；两种形态共用底部同一行操作。没有选区时整个托盘不可用。
 */
function SelectionTray({
  picker,
  expanded,
  onExpandedChange,
  onSendTo,
  onLayout,
  bottomInset,
  animateLayout,
  colors,
}: SelectionTrayProps) {
  const { height: screenH } = useWindowDimensions();
  const keyboardHeight = useKeyboardHeight();
  // 键盘弹出时托盘贴在键盘上，不再需要为导航栏留白
  const bottomPadding = keyboardHeight > 0 ? 0 : bottomInset;
  const expandedHeight = Math.min(470, Math.round(screenH * 0.62));

  return (
    <Animated.View
      testID="word-picker-tray"
      layout={animateLayout ? LinearTransition.duration(220) : undefined}
      onLayout={onLayout}
      style={[
        s.tray,
        {
          backgroundColor: colors.surfaceHigh,
          bottom: keyboardHeight,
          paddingBottom: bottomPadding + 16,
        },
        expanded && { height: expandedHeight + bottomPadding },
      ]}
    >
      <View style={[s.grip, { backgroundColor: colors.border }]} />
      {expanded ? (
        <ExpandedTrayContent
          picker={picker}
          onCollapse={() => onExpandedChange(false)}
          colors={colors}
        />
      ) : (
        <CollapsedTrayContent
          picker={picker}
          onExpand={() => onExpandedChange(true)}
          colors={colors}
        />
      )}
      <TrayActions
        picker={picker}
        disabled={picker.selectedCount === 0}
        onSendTo={onSendTo}
        colors={colors}
      />
    </Animated.View>
  );
}

/** 收起态：选区统计 + 清除，以及两行截断的预览（点按展开） */
function CollapsedTrayContent({
  picker,
  onExpand,
  colors,
}: {
  picker: Picker;
  onExpand: () => void;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('history');
  const hasSelection = picker.selectedCount > 0;
  const foreground = hasSelection ? colors.textPrimary : colors.textSecondary;
  return (
    <>
      <View style={s.summaryRow}>
        <Text
          testID="word-picker-summary"
          style={[m3Type.labelLarge, s.flexFill, { color: foreground }]}
          numberOfLines={1}
        >
          {selectionSummary(t, picker)}
        </Text>
        {hasSelection ? (
          <TextButton
            testID="word-picker-clear"
            label={t('action.clear', { ns: 'common' })}
            onPress={picker.clearSelection}
            colors={colors}
            compact
          />
        ) : null}
      </View>
      <Pressable
        testID="word-picker-preview"
        onPress={onExpand}
        disabled={!hasSelection}
        accessibilityRole="button"
        accessibilityLabel={t('wordPicker.expandPreview')}
        accessibilityState={{ disabled: !hasSelection }}
        android_ripple={{ color: colors.fillSecondary as string }}
        style={[s.preview, { backgroundColor: colors.surface }]}
      >
        <Text style={[s.previewText, { color: foreground }]} numberOfLines={2} ellipsizeMode="tail">
          {hasSelection ? picker.outputText : t('wordPicker.placeholder')}
        </Text>
        {hasSelection ? (
          <Ionicons name="chevron-up" size={20} color={colors.textSecondary} />
        ) : null}
      </Pressable>
    </>
  );
}

/** 展开态：可直接编辑的复制前预览；选区有多个连续段时给出拼接方式 */
function ExpandedTrayContent({
  picker,
  onCollapse,
  colors,
}: {
  picker: Picker;
  onCollapse: () => void;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('history');
  return (
    <>
      <View style={s.expandedHeader}>
        <Text style={[m3Type.titleLarge, s.flexFill, { color: colors.textPrimary }]}>
          {t('wordPicker.previewTitle')}
        </Text>
        <M3IconButton
          testID="word-picker-collapse"
          icon="chevron-down"
          accessibilityLabel={t('wordPicker.collapsePreview')}
          onPress={onCollapse}
          colors={colors}
        />
      </View>
      <View style={s.editorBlock}>
        <Text style={[m3Type.labelMedium, { color: colors.accent }]}>
          {t('wordPicker.editable')}
        </Text>
        <TextInput
          testID="word-picker-editor"
          value={picker.outputText}
          onChangeText={picker.setOutputText}
          multiline
          textAlignVertical="top"
          selectionColor={colors.accent}
          cursorColor={colors.accent}
          style={[
            s.editor,
            {
              borderColor: colors.accent,
              backgroundColor: colors.surface,
              color: colors.textPrimary,
            },
          ]}
        />
      </View>
      {picker.runs.length > 1 ? (
        <View style={s.joinBlock}>
          <Text style={[m3Type.labelMedium, { color: colors.textSecondary }]}>
            {t('wordPicker.joinLabel')}
          </Text>
          <View style={s.joinChips}>
            {JOIN_MODES.map((mode) => (
              <FilterChip
                key={mode}
                testID={`word-picker-join-${mode}`}
                label={t(`wordPicker.join.${mode}`)}
                selected={picker.joinMode === mode}
                onPress={() => picker.setJoinMode(mode)}
              />
            ))}
          </View>
        </View>
      ) : null}
    </>
  );
}

/** 「已选 N 个词 · M 字」；逐字粒度只报字数，没有选区时为「未选择」 */
function selectionSummary(t: TFunction<'history'>, picker: Picker): string {
  if (picker.selectedCount === 0) return t('wordPicker.noneSelected');
  const chars = t('wordPicker.summary.chars', { count: picker.charCount });
  if (picker.granularity === 'char') {
    return t('wordPicker.summary.charsOnly', { count: picker.charCount });
  }
  return `${t('wordPicker.summary.words', { count: picker.selectedCount })} · ${chars}`;
}

/**
 * 托盘底部操作行：联网搜索、分享、发送到（宿主提供时）与复制。
 * 收起与展开两种形态共用这一行，保证入口一致。
 */
function TrayActions({
  picker,
  disabled,
  onSendTo,
  colors,
}: {
  picker: Picker;
  disabled: boolean;
  onSendTo?: (text: string) => void;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('history');
  return (
    <View style={s.actionRow}>
      <M3IconButton
        testID="word-picker-search"
        icon="globe-outline"
        variant="tonal"
        size="large"
        accessibilityLabel={t('wordPicker.webSearch')}
        onPress={picker.searchSelected}
        disabled={disabled}
        colors={colors}
        iconColor={colors.textPrimary}
      />
      <M3IconButton
        testID="word-picker-share"
        icon="share-social-outline"
        variant="tonal"
        size="large"
        accessibilityLabel={t('action.share', { ns: 'common' })}
        onPress={picker.shareSelected}
        disabled={disabled}
        colors={colors}
        iconColor={colors.textPrimary}
      />
      {onSendTo ? (
        <M3IconButton
          testID="word-picker-send-to"
          icon="paper-plane-outline"
          variant="tonal"
          size="large"
          accessibilityLabel={t('detail.sendTo', { ns: 'home' })}
          onPress={() => onSendTo(picker.outputText)}
          disabled={disabled}
          colors={colors}
          iconColor={colors.textPrimary}
        />
      ) : null}
      <CopyButton onPress={picker.copySelected} disabled={disabled} colors={colors} />
    </View>
  );
}

/** Modal 窗口不随软键盘缩放（edge-to-edge）：托盘自己抬到键盘之上 */
function useKeyboardHeight(): number {
  const [height, setHeight] = useState(0);
  useEffect(() => {
    // endCoordinates.height 不含键盘下方的导航栏；按键盘顶边到屏幕底算实际遮挡
    const show = Keyboard.addListener('keyboardDidShow', (e) =>
      setHeight(Math.max(0, Dimensions.get('screen').height - e.endCoordinates.screenY))
    );
    const hide = Keyboard.addListener('keyboardDidHide', () => setHeight(0));
    return () => {
      show.remove();
      hide.remove();
    };
  }, []);
  return height;
}

function CopyButton({
  onPress,
  disabled,
  colors,
}: {
  onPress: () => void;
  disabled: boolean;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('common');
  const fg = disabled ? colors.textDisabled : colors.onAccent;
  return (
    <Pressable
      testID="word-picker-copy"
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityState={{ disabled }}
      android_ripple={{ color: colors.fillSecondary as string }}
      style={[s.copyButton, { backgroundColor: disabled ? colors.surfaceHighest : colors.accent }]}
    >
      <Ionicons name="copy-outline" size={20} color={fg} />
      <Text style={[m3Type.labelLarge, s.copyLabel, { color: fg }]}>{t('action.copy')}</Text>
    </Pressable>
  );
}

/** M3 text button：48dp 触控高度，40dp（compact 为 32dp）可见 state layer */
function TextButton({
  label,
  onPress,
  disabled = false,
  compact = false,
  colors,
  testID,
}: {
  label: string;
  onPress: () => void;
  disabled?: boolean;
  compact?: boolean;
  colors: ColorScheme;
  testID?: string;
}) {
  return (
    <Pressable
      testID={testID}
      onPress={onPress}
      disabled={disabled}
      hitSlop={compact ? { top: 8, bottom: 8 } : undefined}
      accessibilityRole="button"
      accessibilityState={{ disabled }}
      android_ripple={{ color: colors.fillSecondary as string }}
      style={[s.textButton, compact && s.textButtonCompact]}
    >
      <Text
        style={[m3Type.labelLarge, { color: disabled ? colors.textDisabled : colors.accent }]}
        numberOfLines={1}
      >
        {label}
      </Text>
    </Pressable>
  );
}

/** M3 outlined segmented button（单选）：选中段填 secondary container 并带对勾 */
function GranularityToggle({
  value,
  onChange,
  colors,
}: {
  value: 'word' | 'char';
  onChange: (g: 'word' | 'char') => void;
  colors: ColorScheme;
}) {
  const { t } = useTranslation('history');
  return (
    <View accessibilityRole="radiogroup" style={[s.segTrack, { borderColor: colors.border }]}>
      {(['word', 'char'] as const).map((g, i) => {
        const active = value === g;
        const fg = active ? colors.onAccentContainer : colors.textPrimary;
        return (
          <Pressable
            key={g}
            testID={`word-picker-granularity-${g}`}
            onPress={() => onChange(g)}
            android_ripple={{ color: colors.fillSecondary as string }}
            style={[
              s.segItem,
              i === 0 && [s.segDivider, { borderRightColor: colors.border }],
              active && { backgroundColor: colors.accentContainer },
            ]}
            accessibilityRole="radio"
            accessibilityState={{ checked: active }}
          >
            {active ? <Ionicons name="checkmark" size={18} color={fg} /> : null}
            <Text style={[m3Type.labelLarge, { color: fg }]}>
              {g === 'word' ? t('wordPicker.granularity.word') : t('wordPicker.granularity.char')}
            </Text>
          </Pressable>
        );
      })}
    </View>
  );
}

function SkeletonRows({ colors }: { colors: ColorScheme }) {
  return (
    <View style={s.skeletonWrap}>
      {(['92%', '78%', '86%', '60%'] as const).map((width) => (
        <View key={width} style={[s.skeletonBar, { width, backgroundColor: colors.surfaceMid }]} />
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
 * 词流中的一项：词块或标点（空白由外层处理：普通空格不占位，含换行的空白渲染为 LineBreak）。
 * 每项左侧带一个 TILE_GAP 宽的前导区作为间隔：与前一项连成高亮带时填成强调色，
 * 让相邻选中词读起来是一整段；行首项的前导区落在流容器的负边距里，始终透明。
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
  const rightRadius = inBand && joinNext ? 0 : 10;
  const corners = {
    borderTopLeftRadius: joined ? 0 : 10,
    borderBottomLeftRadius: joined ? 0 : 10,
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
        {joined && !token.isPunctuation ? (
          <View style={[s.divider, { backgroundColor: alpha(colors.onAccent, 0.28) }]} />
        ) : null}
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
      {startHandle ? (
        <SelectionHandle side="start" label={t('wordPicker.handleStart')} colors={colors} />
      ) : null}
      {endHandle ? (
        <SelectionHandle side="end" label={t('wordPicker.handleEnd')} colors={colors} />
      ) : null}
    </View>
  );
});

type Corners = Pick<
  ViewStyle,
  | 'borderTopLeftRadius'
  | 'borderBottomLeftRadius'
  | 'borderTopRightRadius'
  | 'borderBottomRightRadius'
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
        { backgroundColor: inBand ? colors.accent : colors.surfaceMid },
        pressed && s.tilePressed,
      ]}
    >
      <Text
        style={[s.tileText, { color: inBand ? colors.onAccent : colors.textPrimary }]}
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
      <Text style={[s.tileText, { color: inBand ? colors.onAccent : colors.textSecondary }]}>
        {text}
      </Text>
    </View>
  );
}

/** 含换行的空白：强制断行；连续空行按行数加一点额外间距（最多两行） */
function LineBreak({ text }: { text: string }) {
  const extraLines = text.split('\n').length - 2;
  return <View style={[s.lineBreak, { height: Math.min(extraLines, 2) * 12 }]} />;
}

/** 选区首 / 尾拖柄（纯展示；拖动由 useWordPicker 的手势按同一几何命中） */
function SelectionHandle({
  side,
  label,
  colors,
}: {
  side: 'start' | 'end';
  label: string;
  colors: ColorScheme;
}) {
  return (
    <View
      testID={`word-picker-handle-${side}`}
      accessibilityLabel={label}
      pointerEvents="none"
      style={[s.handle, side === 'start' ? s.handleStart : s.handleEnd]}
    >
      <View style={[s.handleStem, { backgroundColor: colors.accent }]} />
      <View style={[s.handleKnob, { backgroundColor: colors.accent }]} />
    </View>
  );
}

const s = StyleSheet.create({
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
  textButton: {
    height: 40,
    paddingHorizontal: 12,
    marginRight: 8,
    borderRadius: 20,
    overflow: 'hidden',
    alignItems: 'center',
    justifyContent: 'center',
  },
  textButtonCompact: {
    height: 32,
    marginRight: 0,
    borderRadius: 16,
  },
  modeRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    paddingHorizontal: 16,
    paddingTop: 4,
    paddingBottom: 12,
  },
  deviceLabel: {
    flexShrink: 1,
  },
  segTrack: {
    flexDirection: 'row',
    width: 200,
    borderWidth: 1,
    borderRadius: 20,
    overflow: 'hidden',
  },
  segDivider: {
    borderRightWidth: 1,
  },
  segItem: {
    flex: 1,
    height: 40,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
  },
  hintRow: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
    paddingHorizontal: 16,
    paddingBottom: 16,
  },
  hintChip: {
    height: 28,
    paddingHorizontal: 10,
    borderRadius: 8,
    justifyContent: 'center',
  },
  banner: {
    paddingHorizontal: 20,
    paddingBottom: 6,
  },
  scroll: {
    flex: 1,
  },
  scrollContent: {
    paddingTop: 4,
    paddingHorizontal: 16,
  },
  flow: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    rowGap: 10,
    // 抵消每项的前导区，使行首词块与页边距对齐
    marginLeft: -TILE_GAP,
  },
  item: {
    flexDirection: 'row',
    maxWidth: '80%',
  },
  // 连成带时整项（前导区 + 本体）铺带色，并向左多盖 1px，
  // 盖住相邻两项、前导区与本体之间取整留下的亚像素缝；净宽不变，布局不动
  itemJoined: {
    marginLeft: -1,
    paddingLeft: 1,
  },
  itemRaised: {
    zIndex: 1,
  },
  lead: {
    width: TILE_GAP,
    height: TILE_HEIGHT,
    alignItems: 'center',
    justifyContent: 'center',
  },
  divider: {
    width: 1,
    height: 20,
  },
  lineBreak: {
    width: '100%',
  },
  tile: {
    height: TILE_HEIGHT,
    paddingHorizontal: 10,
    justifyContent: 'center',
    flexShrink: 1,
  },
  tileChar: {
    minWidth: TILE_HEIGHT,
    alignItems: 'center',
  },
  tilePressed: {
    opacity: 0.8,
  },
  tileText: {
    fontSize: 17,
    lineHeight: 22,
  },
  punct: {
    height: TILE_HEIGHT,
    minWidth: 14,
    alignItems: 'center',
    justifyContent: 'center',
  },
  handle: {
    position: 'absolute',
    top: TILE_HEIGHT - 2,
    width: 16,
    alignItems: 'center',
  },
  // 拖柄宽 16，中线对齐本体的左 / 右边缘
  handleStart: {
    left: TILE_GAP - 8,
  },
  handleEnd: {
    right: -8,
  },
  handleStem: {
    width: 4,
    height: 8,
  },
  handleKnob: {
    width: 16,
    height: 16,
    borderRadius: 8,
  },
  skeletonWrap: {
    flex: 1,
    padding: 16,
    gap: 12,
  },
  skeletonBar: {
    height: TILE_HEIGHT,
    borderRadius: 10,
  },
  emptyWrap: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
  },
  tray: {
    position: 'absolute',
    left: 0,
    right: 0,
    borderTopLeftRadius: 28,
    borderTopRightRadius: 28,
    paddingTop: 10,
    paddingHorizontal: 16,
    gap: 12,
    boxShadow: '0px -2px 8px rgba(0, 0, 0, 0.06)',
  },
  grip: {
    alignSelf: 'center',
    width: 32,
    height: 4,
    borderRadius: 2,
    opacity: 0.5,
  },
  summaryRow: {
    height: 32,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  flexFill: {
    flex: 1,
  },
  preview: {
    minHeight: 52,
    borderRadius: 14,
    paddingVertical: 10,
    paddingLeft: 14,
    paddingRight: 12,
    overflow: 'hidden',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  previewText: {
    flex: 1,
    fontSize: 15,
    lineHeight: 22,
  },
  actionRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  copyButton: {
    flex: 1,
    height: 48,
    borderRadius: 24,
    overflow: 'hidden',
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
  },
  copyLabel: {
    fontSize: 15,
  },
  expandedHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    marginRight: -8,
  },
  editorBlock: {
    flex: 1,
    minHeight: 0,
    gap: 6,
  },
  editor: {
    flex: 1,
    borderWidth: 2,
    borderRadius: 12,
    paddingHorizontal: 16,
    paddingVertical: 14,
    fontSize: 17,
    lineHeight: 26,
  },
  joinBlock: {
    gap: 8,
  },
  joinChips: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    gap: 8,
  },
});
