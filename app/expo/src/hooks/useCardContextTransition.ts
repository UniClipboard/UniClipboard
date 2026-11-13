import { useCallback, useEffect, useRef, useState } from 'react';
import { useWindowDimensions, View, type LayoutChangeEvent } from 'react-native';
import {
  Easing,
  interpolate,
  useAnimatedStyle,
  useReducedMotion,
  useSharedValue,
  withSpring,
  withTiming,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type { CardAnchorRect } from '@/components/CardContextOverlay.types';
import { overlayMotion } from '@/theme/motion';

const STACK_MARGIN = 24;

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

/**
 * 卡片长按浮层的进出场动画与定位，iOS/Android 两个渲染层共用。
 *
 * 浮层栈（预览 + 菜单）先以 opacity 0 渲染并自测量，然后从卡片原位（anchor）
 * 缩放/位移弹入最终位置，退场反向收回。全程只有长按那一刻的一次
 * measureInWindow，不给网格 cell 包任何原生视图——这是对 da2635b 修掉的
 * RNHostView 挂载期几何 flicker 的规避。
 */
export function useCardContextTransition(anchor: CardAnchorRect | null, onDismiss: () => void) {
  const { width: screenW, height: screenH } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const reducedMotion = useReducedMotion();

  const progress = useSharedValue(0);
  const closingRef = useRef(false);
  const enteredRef = useRef(false);
  const rootRef = useRef<View>(null);

  // anchor 是卡片在 Activity/主窗口里的坐标；浮层挂在 Modal（独立窗口）里，
  // 再测一次自身窗口原点做归一，抵消两个窗口原点可能的差异（Android 状态栏）
  const [localAnchor, setLocalAnchor] = useState<CardAnchorRect | null>(null);
  useEffect(() => {
    if (!anchor) {
      setLocalAnchor(null);
      return;
    }
    const node = rootRef.current;
    if (!node) {
      setLocalAnchor(anchor);
      return;
    }
    node.measureInWindow((x, y) => {
      setLocalAnchor({ ...anchor, x: anchor.x - (x || 0), y: anchor.y - (y || 0) });
    });
  }, [anchor]);

  const [stackSize, setStackSize] = useState<{ width: number; height: number } | null>(null);
  const onStackLayout = useCallback((e: LayoutChangeEvent) => {
    const { width, height } = e.nativeEvent.layout;
    setStackSize((prev) =>
      prev && Math.abs(prev.width - width) < 1 && Math.abs(prev.height - height) < 1
        ? prev
        : { width, height }
    );
  }, []);

  useEffect(() => {
    if (!stackSize || enteredRef.current) return;
    enteredRef.current = true;
    progress.value = reducedMotion
      ? withTiming(1, { duration: overlayMotion.reducedMotionDuration })
      : withSpring(1, overlayMotion.enterSpring);
  }, [stackSize, reducedMotion, progress]);

  // 水平：与卡片同侧列对齐；垂直：尽量贴卡片顶，越界则收进安全区
  const side: 'left' | 'right' =
    localAnchor && localAnchor.x + localAnchor.width / 2 > screenW / 2 ? 'right' : 'left';
  const topMin = insets.top + 12;
  const bottomMax = screenH - insets.bottom - 16;
  const stackTop = stackSize
    ? clamp(
        localAnchor ? localAnchor.y : (screenH - stackSize.height) / 2,
        topMin,
        Math.max(topMin, bottomMax - stackSize.height)
      )
    : topMin;

  const hasLayout = stackSize !== null;
  const finalCenterX = stackSize
    ? side === 'left'
      ? STACK_MARGIN + stackSize.width / 2
      : screenW - STACK_MARGIN - stackSize.width / 2
    : screenW / 2;
  const finalCenterY = stackTop + (stackSize?.height ?? 0) / 2;
  const fromX = localAnchor ? localAnchor.x + localAnchor.width / 2 : finalCenterX;
  const fromY = localAnchor ? localAnchor.y + localAnchor.height / 2 : finalCenterY;
  const dx = fromX - finalCenterX;
  const dy = fromY - finalCenterY;
  const startScale =
    localAnchor && stackSize ? clamp(localAnchor.width / stackSize.width, 0.5, 0.92) : 0.9;

  const scrimStyle = useAnimatedStyle(() => ({ opacity: progress.value }), []);
  const stackStyle = useAnimatedStyle(() => {
    if (!hasLayout) return { opacity: 0 };
    const p = progress.value;
    if (reducedMotion) return { opacity: p };
    return {
      opacity: interpolate(p, [0, 0.35, 1], [0, 1, 1]),
      transform: [
        { translateX: dx * (1 - p) },
        { translateY: dy * (1 - p) },
        { scale: startScale + (1 - startScale) * p },
      ],
    };
  }, [hasLayout, reducedMotion, dx, dy, startScale]);

  /** 播放退场动画，结束后触发 onDismiss；after 用于"选中动作后再执行"的时序 */
  const close = useCallback(
    (after?: () => void) => {
      if (closingRef.current) return;
      closingRef.current = true;
      const finish = () => {
        onDismiss();
        // 把选中动作推迟到浮层的原生 <Modal> 真正卸载/dismiss 之后再执行。
        // 若在同一 tick 里紧接着 present 系统面板(如分享 sheet),会在
        // iOS 16.x 上构成 present-while-dismissing 而冻结整个界面。浮层是
        // animationType="none",卸载几乎瞬时,隔两帧即可让 dismiss 完成。
        if (after) {
          requestAnimationFrame(() => requestAnimationFrame(after));
        }
      };
      progress.value = withTiming(
        0,
        { duration: overlayMotion.exitDuration, easing: Easing.in(Easing.quad) },
        () => {
          scheduleOnRN(finish);
        }
      );
    },
    [onDismiss, progress]
  );

  return {
    rootRef,
    side,
    stackTop,
    margin: STACK_MARGIN,
    previewMaxWidth: Math.min(screenW - STACK_MARGIN * 2 - 16, 340),
    previewMaxHeight: Math.round(screenH * 0.55),
    onStackLayout,
    scrimStyle,
    stackStyle,
    close,
  };
}
