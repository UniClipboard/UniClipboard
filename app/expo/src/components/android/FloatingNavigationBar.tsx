import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Pressable,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
  type LayoutChangeEvent,
} from 'react-native';
import MaterialIcons from '@expo/vector-icons/MaterialIcons';
import * as Haptics from 'expo-haptics';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  Easing,
  useAnimatedProps,
  useAnimatedStyle,
  useDerivedValue,
  useSharedValue,
  withSpring,
  withTiming,
  type SharedValue,
} from 'react-native-reanimated';
import { scheduleOnRN, scheduleOnUI } from 'react-native-worklets';
import type { EdgeInsets } from 'react-native-safe-area-context';
import { FAB_SIZE } from '@/components/AddActionsFab.types';
import type { MainTabParamList } from '@/navigation/AppNavigator.types';
import { m3Type } from '@/theme/m3Typography';
import { FLOATING_NAV_HEIGHT, FLOATING_NAV_MARGIN } from './mainNavigationMetrics';

/** 与 rail 的 Compose 矢量图标同一套 Material 字形。 */
const ICONS: Record<keyof MainTabParamList, keyof typeof MaterialIcons.glyphMap> = {
  Clipboard: 'content-paste',
  Devices: 'devices',
  Preferences: 'settings',
};

const PILL_PADDING = 8;
const ITEM_GAP = 4;
const ITEM_HEIGHT = FLOATING_NAV_HEIGHT - PILL_PADDING * 2;
const ICON_SIZE = 24;
/** 收起态:48dp 方形,图标居中。 */
const ITEM_MIN_WIDTH = ITEM_HEIGHT;
const COLLAPSED_PADDING = (ITEM_MIN_WIDTH - ICON_SIZE) / 2;
/** 展开态:左 16 + 图标 24 + 间距 8 + 标签 + 右 20。 */
const EXPANDED_PADDING_START = 16;
const EXPANDED_PADDING_END = 20;
const LABEL_GAP = 8;
/** 展开态相对收起态多出的固定宽度(不含标签)。 */
const EXPANDED_EXTRA =
  EXPANDED_PADDING_START + LABEL_GAP + EXPANDED_PADDING_END - COLLAPSED_PADDING;
/** 胶囊与右下 FAB 之间保留的最小间距(dp)。 */
const FAB_GAP = 12;
/** 手指横向移动超过该距离才进入拖拽切换,更短的触摸交给各目的地的点按。 */
const DRAG_ACTIVATION_DISTANCE = 8;
/** 选中位置、指示器、各项宽度共用一条弹簧,保证整体同步。 */
const SPRING = { damping: 26, stiffness: 300, mass: 0.9 };
/** 收起时下沉并略微缩小;出现用弹簧,收起用短促的加速曲线,先于菜单遮罩让开。 */
const HIDDEN_OFFSET_Y = 24;
const HIDDEN_SCALE = 0.9;
const SHOW_SPRING = { damping: 20, stiffness: 260, mass: 0.9 };
const HIDE_TIMING = { duration: 140, easing: Easing.in(Easing.quad) };
/** 拖拽中指示器略微放大,提示它已被「拿起」。 */
const DRAG_INDICATOR_SCALE = 0.06;
/** 标签在展开程度超过该值后才淡入,此时目的地已腾出足够空间,不压到相邻项。 */
const LABEL_REVEAL = 0.6;
/** 胶囊阴影在裁切容器内预留的外扩空间(dp)。 */
const SHADOW_BLEED = 8;
/** 指示器中段的布局宽度;实际宽度由 scaleX 得出。 */
const INDICATOR_MIDDLE_BASE = 100;

type Rect = { x: number; width: number };

export interface FloatingNavigationItem {
  key: string;
  name: keyof MainTabParamList;
  label: string;
  selected: boolean;
  onPress: () => void;
}

export interface FloatingNavigationColors {
  container: string;
  indicator: string;
  onIndicator: string;
  inactive: string;
}

interface FloatingNavigationBarProps {
  items: FloatingNavigationItem[];
  colors: FloatingNavigationColors;
  insets: EdgeInsets;
  /** 首页搜索 / 多选时收起。 */
  hidden: boolean;
  /** 首页添加菜单展开时收起(UI 线程驱动,不经 React 重渲)。 */
  hiddenByMenu: SharedValue<boolean>;
}

/**
 * 一次选中切换的过渡状态:从切换发起时各项的展开程度 / 指示器矩形,过渡到目标项
 * 独占展开。只有起点项收起、目标项展开,途经的项不受影响;中途再次切换从当下状态衔接。
 */
type Motion = { startExpansion: number[]; startRect: Rect; target: number };

/** 目的地 i 的展开程度(0 收起,1 完全展开)。 */
function expansion(i: number, motion: Motion, progress: number): number {
  'worklet';
  const start = motion.startExpansion[i] ?? 0;
  return start + ((i === motion.target ? 1 : 0) - start) * progress;
}

/** 目的地 i 在胶囊内的横向位置(相对胶囊左缘)。 */
function itemRect(i: number, motion: Motion, progress: number, extras: number[]): Rect {
  'worklet';
  let x = PILL_PADDING;
  let width = 0;
  for (let j = 0; j <= i; j++) {
    width = ITEM_MIN_WIDTH + expansion(j, motion, progress) * (extras[j] ?? 0);
    if (j < i) x += width + ITEM_GAP;
  }
  return { x, width };
}

/** 静止指示器:从切换起点的矩形滑向目标项(目标项此时也在展开)。 */
function restingIndicator(motion: Motion, progress: number, extras: number[]): Rect {
  'worklet';
  const to = itemRect(motion.target, motion, progress, extras);
  const from = motion.startRect;
  return {
    x: from.x + (to.x - from.x) * progress,
    width: from.width + (to.width - from.width) * progress,
  };
}

/** 胶囊当前的可见宽度:末项右缘加内边距。 */
function visiblePillWidth(motion: Motion, progress: number, extras: number[]): number {
  'worklet';
  const last = itemRect(extras.length - 1, motion, progress, extras);
  return last.x + last.width + PILL_PADDING;
}

/** 以 center 为原点按 scale 缩放后,原中心在 x 处的一段的新中心。 */
function scaledAbout(x: number, center: number, scale: number): number {
  'worklet';
  return center + (x - center) * scale;
}

/** 手指所在(或最近)的目的地下标。 */
function hitTest(x: number, motion: Motion, progress: number, extras: number[]): number {
  'worklet';
  let nearest = 0;
  let nearestDistance = Infinity;
  for (let i = 0; i < extras.length; i++) {
    const rect = itemRect(i, motion, progress, extras);
    const distance = Math.abs(x - (rect.x + rect.width / 2));
    if (distance < nearestDistance) {
      nearest = i;
      nearestDistance = distance;
    }
  }
  return nearest;
}

function settledMotion(index: number, count: number): Motion {
  return {
    startExpansion: Array.from({ length: count }, (_, i) => (i === index ? 1 : 0)),
    startRect: { x: 0, width: 0 },
    target: index,
  };
}

function tickHaptic() {
  Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Segment_Tick).catch(() => {});
}

/**
 * 手机顶级导航:M3 Expressive 悬浮胶囊。贴左下浮在内容之上,右侧让给首页 FAB,
 * 二者垂直居中对齐成一组。选中目的地展开为「图标 + 标签」,其余只显示图标。
 *
 * 动画只由一个过渡进度驱动:各项位置、标签透明度、指示器位置与宽度都由它算出,
 * 因此始终同步;只有起点项收起、目标项展开,途经的项不动。点按 / 松手即刻起动画,
 * 不等导航完成。
 * 除点按外支持拖拽切换:按住胶囊横向滑动,指示器跟随手指并在经过的目的地上高亮
 * (刻度震动),松手后从手指处平滑落到目标上并切换。
 * 逐帧只改 transform / opacity,不动布局属性:胶囊与指示器由圆头 + 平移 / 横向缩放的
 * 段拼成,各项绝对定位后平移。这样每帧无需重新布局、重绘,不与切换页面的挂载抢主线程。
 * 布局尺寸(胶囊触控区、各项点按区)只在选中目标变化时按静止态更新一次。
 */
export function FloatingNavigationBar({
  items,
  colors,
  insets,
  hidden,
  hiddenByMenu,
}: FloatingNavigationBarProps) {
  const { width } = useWindowDimensions();
  const maxWidth =
    width - insets.left - insets.right - FLOATING_NAV_MARGIN * 2 - FAB_SIZE - FAB_GAP;
  const selectedIndex = Math.max(
    0,
    items.findIndex((item) => item.selected)
  );

  // 标签的自然宽度;长标签截到胶囊不压 FAB 为止
  const [labelWidths, setLabelWidths] = useState<(number | undefined)[]>([]);
  const maxLabelWidth = Math.max(
    0,
    maxWidth -
      PILL_PADDING * 2 -
      (items.length - 1) * (ITEM_MIN_WIDTH + ITEM_GAP) -
      ITEM_MIN_WIDTH -
      EXPANDED_EXTRA
  );
  const measured = items.every((_, i) => labelWidths[i] !== undefined);
  const clampedLabelWidths = useMemo(
    () => items.map((_, i) => Math.min(labelWidths[i] ?? 0, maxLabelWidth)),
    [items, labelWidths, maxLabelWidth]
  );

  const extras = useSharedValue<number[]>([]);
  useEffect(() => {
    extras.value = clampedLabelWidths.map((labelWidth) => EXPANDED_EXTRA + labelWidth);
  }, [extras, clampedLabelWidths]);

  const motion = useSharedValue<Motion>(settledMotion(selectedIndex, items.length));
  const progress = useSharedValue(1);
  // 拖拽:指示器在「静止位置」与「手指位置」之间按 drag 混合
  const drag = useSharedValue(0);
  const dragX = useSharedValue(0);
  const dragWidth = useSharedValue(0);
  const hovered = useSharedValue(-1);

  /** 从当前实际显示的状态起,过渡到 index 独占展开(UI 线程)。 */
  const retarget = useCallback(
    (index: number) => {
      'worklet';
      const list = extras.value;
      const current = motion.value;
      const p = progress.value;
      const rest = restingIndicator(current, p, list);
      const d = drag.value;
      motion.value = {
        startExpansion: list.map((_, i) => expansion(i, current, p)),
        startRect: {
          x: rest.x + (dragX.value - rest.x) * d,
          width: rest.width + (dragWidth.value - rest.width) * d,
        },
        target: index,
      };
      drag.value = 0;
      progress.value = 0;
      progress.value = withSpring(1, SPRING);
    },
    [drag, dragWidth, dragX, extras, motion, progress]
  );

  // 已发起动画的目标;导航完成后 selectedIndex 追上时不再重复起动画
  const targetRef = useRef(selectedIndex);
  const [highlightTarget, setHighlightTarget] = useState(selectedIndex);
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);

  const settle = useCallback((index: number) => {
    targetRef.current = index;
    setHighlightTarget(index);
  }, []);

  const moveTo = useCallback(
    (index: number) => {
      if (targetRef.current === index) return;
      settle(index);
      scheduleOnUI(retarget, index);
    },
    [retarget, settle]
  );

  // 外部导航(如通知跳转)改变选中项时同步
  useEffect(() => {
    moveTo(selectedIndex);
  }, [moveTo, selectedIndex]);

  const navigate = useCallback(
    (index: number) => {
      const item = items[index];
      if (item && !item.selected) item.onPress();
    },
    [items]
  );

  const activate = useCallback(
    (index: number) => {
      moveTo(index);
      navigate(index);
    },
    [moveTo, navigate]
  );

  const hover = useCallback((index: number) => {
    setHoveredIndex(index);
    tickHaptic();
  }, []);

  const endDrag = useCallback(
    (index: number) => {
      setHoveredIndex(null);
      if (index < 0) return;
      settle(index);
      navigate(index);
    },
    [navigate, settle]
  );

  const dragGesture = Gesture.Pan()
    .activeOffsetX([-DRAG_ACTIVATION_DISTANCE, DRAG_ACTIVATION_DISTANCE])
    .failOffsetY([-24, 24])
    .onStart((event) => {
      const list = extras.value;
      const rest = restingIndicator(motion.value, progress.value, list);
      dragX.value = rest.x;
      dragWidth.value = rest.width;
      drag.value = withSpring(1, SPRING);
      hovered.value = hitTest(event.x, motion.value, progress.value, list);
    })
    .onUpdate((event) => {
      const list = extras.value;
      const m = motion.value;
      const p = progress.value;
      const index = hitTest(event.x, m, p, list);
      const hoveredRect = itemRect(index, m, p, list);
      const first = itemRect(0, m, p, list);
      const last = itemRect(list.length - 1, m, p, list);
      // 指示器中心跟随手指,宽度过渡到所在目的地的宽度,不越出首尾两项
      dragWidth.value = withSpring(hoveredRect.width, SPRING);
      const w = dragWidth.value;
      dragX.value = Math.min(Math.max(event.x - w / 2, first.x), last.x + last.width - w);
      if (index !== hovered.value) {
        hovered.value = index;
        scheduleOnRN(hover, index);
      }
    })
    .onFinalize((_event, success) => {
      const index = success ? hovered.value : -1;
      hovered.value = -1;
      // 松手:从手指处的指示器直接过渡到目标;取消:指示器回到原位
      if (index >= 0) retarget(index);
      else drag.value = withSpring(0, SPRING);
      scheduleOnRN(endDrag, index);
    });

  // 指示器矩形:静止位置与手指位置按 drag 混合,拖拽中略微放大
  const indicator = useDerivedValue(() => {
    const rest = restingIndicator(motion.value, progress.value, extras.value);
    const d = drag.value;
    return {
      x: rest.x + (dragX.value - rest.x) * d,
      width: rest.width + (dragWidth.value - rest.width) * d,
      scale: 1 + DRAG_INDICATOR_SCALE * d,
    };
  });
  // 指示器 = 两个圆头 + 横向缩放的中段,均以指示器中心为原点缩放
  const indicatorStartStyle = useAnimatedStyle(() => {
    const { x, width: w, scale } = indicator.value;
    const center = x + w / 2;
    return {
      transform: [
        { translateX: scaledAbout(x + ITEM_HEIGHT / 2, center, scale) - ITEM_HEIGHT / 2 },
        { scale },
      ],
    };
  });
  const indicatorEndStyle = useAnimatedStyle(() => {
    const { x, width: w, scale } = indicator.value;
    const center = x + w / 2;
    return {
      transform: [
        { translateX: scaledAbout(x + w - ITEM_HEIGHT / 2, center, scale) - ITEM_HEIGHT / 2 },
        { scale },
      ],
    };
  });
  const indicatorMiddleStyle = useAnimatedStyle(() => {
    const { x, width: w, scale } = indicator.value;
    return {
      transform: [
        { translateX: x + w / 2 - INDICATOR_MIDDLE_BASE / 2 },
        { scaleX: ((w - ITEM_HEIGHT) * scale) / INDICATOR_MIDDLE_BASE },
        { scaleY: scale },
      ],
    };
  });

  const onLabelMeasured = useCallback((index: number, labelWidth: number) => {
    setLabelWidths((previous) => {
      if (previous[index] === labelWidth) return previous;
      const next = previous.slice();
      next[index] = labelWidth;
      return next;
    });
  }, []);

  const highlightedIndex = hoveredIndex ?? highlightTarget;

  // 静止态尺寸:只在选中目标变化时更新一次布局,动画期间不再改动
  const collapsedPillWidth =
    PILL_PADDING * 2 + items.length * ITEM_MIN_WIDTH + (items.length - 1) * ITEM_GAP;
  const expandedWidth = (index: number) => EXPANDED_EXTRA + (clampedLabelWidths[index] ?? 0);
  const settledPillWidth = collapsedPillWidth + expandedWidth(highlightTarget);
  let settledX = PILL_PADDING;
  const hitRects = items.map((_, index): Rect => {
    const rect = {
      x: settledX,
      width: ITEM_MIN_WIDTH + (index === highlightTarget ? expandedWidth(index) : 0),
    };
    settledX += rect.width + ITEM_GAP;
    return rect;
  });
  const maxPillWidth = collapsedPillWidth + EXPANDED_EXTRA + Math.max(0, ...clampedLabelWidths);
  // 胶囊主体按最宽静止态布局,平移到右缘与当前可见宽度对齐;左端藏在裁切区外
  const pillBodyStyle = useAnimatedStyle(() => ({
    transform: [
      {
        translateX:
          visiblePillWidth(motion.value, progress.value, extras.value) -
          maxPillWidth -
          FLOATING_NAV_HEIGHT / 2,
      },
    ],
  }));

  // 收起 / 出现:始终挂载,只在 UI 线程上滑出 + 缩小 + 淡出,避免重挂载时状态重置闪一下
  const hiddenByProp = useSharedValue(hidden);
  useEffect(() => {
    hiddenByProp.value = hidden;
  }, [hidden, hiddenByProp]);
  const shown = useDerivedValue(() =>
    hiddenByProp.value || hiddenByMenu.value
      ? withTiming(0, HIDE_TIMING)
      : withSpring(1, SHOW_SPRING)
  );
  const layerStyle = useAnimatedStyle(() => ({
    opacity: Math.min(1, shown.value),
    transform: [
      { translateY: (1 - shown.value) * HIDDEN_OFFSET_Y },
      { scale: HIDDEN_SCALE + (1 - HIDDEN_SCALE) * shown.value },
    ],
  }));
  const layerProps = useAnimatedProps(() => ({
    pointerEvents: (hiddenByProp.value || hiddenByMenu.value ? 'none' : 'box-none') as
      | 'none'
      | 'box-none',
  }));

  return (
    <Animated.View
      animatedProps={layerProps}
      style={[
        styles.layer,
        {
          left: insets.left + FLOATING_NAV_MARGIN,
          bottom: insets.bottom + FLOATING_NAV_MARGIN,
        },
        layerStyle,
      ]}
    >
      {/* 离屏测量各标签的自然宽度 */}
      <View pointerEvents="none" style={styles.measure}>
        {items.map((item, index) => (
          <Text
            key={item.key}
            numberOfLines={1}
            style={styles.label}
            onLayout={(event: LayoutChangeEvent) =>
              onLabelMeasured(index, Math.ceil(event.nativeEvent.layout.width))
            }
          >
            {item.label}
          </Text>
        ))}
      </View>

      <GestureDetector gesture={dragGesture}>
        <View
          testID="main-tab-bar"
          accessibilityRole="tablist"
          style={[styles.pill, { width: settledPillWidth }, !measured && styles.unmeasured]}
        >
          {/* 胶囊底:固定的左圆头 + 平移的主体,各自裁在圆头中线处拼接,阴影不重叠 */}
          <View pointerEvents="none" style={styles.pillStartClip}>
            <View style={[styles.pillStartCap, { backgroundColor: colors.container }]} />
          </View>
          <View
            pointerEvents="none"
            style={[
              styles.pillBodyClip,
              { width: maxPillWidth - FLOATING_NAV_HEIGHT / 2 + SHADOW_BLEED },
            ]}
          >
            <Animated.View
              style={[
                styles.pillBody,
                { width: maxPillWidth, backgroundColor: colors.container },
                pillBodyStyle,
              ]}
            />
          </View>

          <Animated.View
            pointerEvents="none"
            style={[styles.indicatorCap, { backgroundColor: colors.indicator }, indicatorStartStyle]}
          />
          <Animated.View
            pointerEvents="none"
            style={[
              styles.indicatorMiddle,
              { backgroundColor: colors.indicator },
              indicatorMiddleStyle,
            ]}
          />
          <Animated.View
            pointerEvents="none"
            style={[styles.indicatorCap, { backgroundColor: colors.indicator }, indicatorEndStyle]}
          />

          {items.map((item, index) => (
            <FloatingNavigationButton
              key={item.key}
              item={item}
              index={index}
              labelWidth={clampedLabelWidths[index]}
              hitRect={hitRects[index]}
              motion={motion}
              progress={progress}
              extras={extras}
              tint={index === highlightedIndex ? colors.onIndicator : colors.inactive}
              onPress={activate}
            />
          ))}
        </View>
      </GestureDetector>
    </Animated.View>
  );
}

function FloatingNavigationButton({
  item,
  index,
  labelWidth,
  hitRect,
  motion,
  progress,
  extras,
  tint,
  onPress,
}: {
  item: FloatingNavigationItem;
  index: number;
  labelWidth: number;
  /** 点按区:取静止态位置与宽度,只随选中目标变化,动画期间不动。 */
  hitRect: Rect;
  motion: SharedValue<Motion>;
  progress: SharedValue<number>;
  extras: SharedValue<number[]>;
  tint: string;
  onPress: (index: number) => void;
}) {
  // 图标与标签整组平移到目的地位置,展开时再随起始内边距右移
  const contentStyle = useAnimatedStyle(() => {
    const m = motion.value;
    const p = progress.value;
    const f = expansion(index, m, p);
    return {
      transform: [
        {
          translateX:
            itemRect(index, m, p, extras.value).x +
            (EXPANDED_PADDING_START - COLLAPSED_PADDING) * f,
        },
      ],
    };
  });
  // 标签在腾出足够空间后才淡入,收起时先淡出,不压到相邻目的地
  const labelStyle = useAnimatedStyle(() => ({
    opacity: Math.max(
      0,
      (expansion(index, motion.value, progress.value) - LABEL_REVEAL) / (1 - LABEL_REVEAL)
    ),
  }));

  return (
    <>
      <Animated.View pointerEvents="none" style={[styles.content, contentStyle]}>
        <View style={styles.icon}>
          <MaterialIcons name={ICONS[item.name]} size={ICON_SIZE} color={tint} />
        </View>
        <Animated.View style={[styles.labelSlot, labelStyle]}>
          <Text numberOfLines={1} style={[styles.label, { width: labelWidth, color: tint }]}>
            {item.label}
          </Text>
        </Animated.View>
      </Animated.View>
      <Pressable
        testID={`main-tab-${item.name}`}
        onPress={() => onPress(index)}
        accessibilityRole="tab"
        accessibilityState={{ selected: item.selected }}
        accessibilityLabel={item.label}
        style={[styles.hit, { left: hitRect.x, width: hitRect.width }]}
      />
    </>
  );
}

const styles = StyleSheet.create({
  layer: {
    position: 'absolute',
  },
  measure: {
    position: 'absolute',
    left: 0,
    top: 0,
    width: 1000,
    alignItems: 'flex-start',
    opacity: 0,
  },
  pill: {
    height: FLOATING_NAV_HEIGHT,
  },
  unmeasured: {
    opacity: 0,
  },
  pillStartClip: {
    position: 'absolute',
    left: -SHADOW_BLEED,
    top: -SHADOW_BLEED,
    width: SHADOW_BLEED + FLOATING_NAV_HEIGHT / 2,
    height: FLOATING_NAV_HEIGHT + SHADOW_BLEED * 2,
    overflow: 'hidden',
  },
  pillStartCap: {
    position: 'absolute',
    left: SHADOW_BLEED,
    top: SHADOW_BLEED,
    width: FLOATING_NAV_HEIGHT,
    height: FLOATING_NAV_HEIGHT,
    borderRadius: FLOATING_NAV_HEIGHT / 2,
    elevation: 3,
  },
  pillBodyClip: {
    position: 'absolute',
    left: FLOATING_NAV_HEIGHT / 2,
    top: -SHADOW_BLEED,
    height: FLOATING_NAV_HEIGHT + SHADOW_BLEED * 2,
    overflow: 'hidden',
  },
  pillBody: {
    position: 'absolute',
    left: 0,
    top: SHADOW_BLEED,
    height: FLOATING_NAV_HEIGHT,
    borderRadius: FLOATING_NAV_HEIGHT / 2,
    elevation: 3,
  },
  indicatorCap: {
    position: 'absolute',
    left: 0,
    top: PILL_PADDING,
    width: ITEM_HEIGHT,
    height: ITEM_HEIGHT,
    borderRadius: ITEM_HEIGHT / 2,
  },
  indicatorMiddle: {
    position: 'absolute',
    left: 0,
    top: PILL_PADDING,
    width: INDICATOR_MIDDLE_BASE,
    height: ITEM_HEIGHT,
  },
  content: {
    position: 'absolute',
    left: 0,
    top: PILL_PADDING,
    width: ITEM_MIN_WIDTH,
    height: ITEM_HEIGHT,
  },
  hit: {
    position: 'absolute',
    top: PILL_PADDING,
    height: ITEM_HEIGHT,
  },
  icon: {
    position: 'absolute',
    left: COLLAPSED_PADDING,
    top: (ITEM_HEIGHT - ICON_SIZE) / 2,
  },
  labelSlot: {
    position: 'absolute',
    left: COLLAPSED_PADDING + ICON_SIZE + LABEL_GAP,
    top: 0,
    height: ITEM_HEIGHT,
    justifyContent: 'center',
  },
  label: {
    ...m3Type.labelLarge,
  },
});
