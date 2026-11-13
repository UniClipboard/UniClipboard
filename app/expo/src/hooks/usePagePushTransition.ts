import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useWindowDimensions } from 'react-native';
import {
  Easing,
  interpolate,
  useAnimatedStyle,
  useReducedMotion,
  useSharedValue,
  withTiming,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { duration, overlayMotion } from '@/theme/motion';

/** 入场起点的水平偏移（占屏宽比例），近似 M3 shared axis X 的前进方向 */
const PUSH_OFFSET = 0.12;

/**
 * 全屏页面的推入 / 退出转场（M3 shared axis X）：从右侧少量位移并淡入，退出反向。
 * close(after) / onEnterComplete 让调用方可以把重活挪到入场之后、把后续动作排在退场之后。
 *
 * pageStyle 只在转场进行中是动画样式，入场结束后为 undefined，页面回到普通的 React 样式。
 * Reanimated 靠 JS 线程上的定时回收把已结束动画的终值交还 React：超过 1 秒才交还、超过 2 秒
 * 直接丢弃。冷启动时 JS 线程常被阻塞 1 秒以上，终值会在交还前被丢弃，之后任意一次重渲染都把
 * 页面打回首帧（透明、右移），盖在下层之上吞掉触摸。静止时不挂动画样式就不依赖这次交还。
 * 入场与退场各用一个动画样式：动画样式挂上时先用首次渲染算出的初值，退场样式的初值是完全可见，
 * 重新挂上不会闪。settled 在入场结束后为 true，布局过渡等依赖稳定布局的动画应等到此时再启用。
 */
export function usePagePushTransition(
  onDismiss: () => void,
  onEnterComplete?: () => void,
  /** 入场起点的水平偏移占屏宽比例:默认为 M3 shared axis 的少量位移,iOS 推入传 1(整屏宽) */
  pushOffset: number = PUSH_OFFSET
) {
  const { width } = useWindowDimensions();
  const reducedMotion = useReducedMotion();
  const enter = useSharedValue(0);
  const exit = useSharedValue(1);
  const [phase, setPhase] = useState<'entering' | 'settled' | 'exiting'>('entering');
  const closingRef = useRef(false);
  const enteredRef = useRef(false);
  const afterRef = useRef<(() => void) | undefined>(undefined);

  const onEnterCompleteRef = useRef(onEnterComplete);
  const onDismissRef = useRef(onDismiss);
  useLayoutEffect(() => {
    onEnterCompleteRef.current = onEnterComplete;
    onDismissRef.current = onDismiss;
  }, [onEnterComplete, onDismiss]);

  useEffect(() => {
    if (enteredRef.current) return;
    enteredRef.current = true;
    const fireEnterComplete = () => {
      if (!closingRef.current) setPhase('settled');
      onEnterCompleteRef.current?.();
    };
    enter.value = withTiming(
      1,
      {
        duration: reducedMotion ? overlayMotion.reducedMotionDuration : duration.slow,
        easing: Easing.bezier(0.05, 0.7, 0.1, 1),
      },
      (finished) => {
        if (finished) scheduleOnRN(fireEnterComplete);
      }
    );
  }, [reducedMotion, enter]);

  // 退场等退场样式挂到视图上之后再开始，头几帧才不会落空
  useEffect(() => {
    if (phase !== 'exiting') return;
    const finish = () => {
      onDismissRef.current();
      afterRef.current?.();
    };
    // 入场中途关闭时从当前可见度退出，不先跳回完全可见
    exit.value = Math.min(exit.value, enter.value);
    exit.value = withTiming(
      0,
      { duration: duration.fast, easing: Easing.bezier(0.3, 0, 0.8, 0.15) },
      () => {
        scheduleOnRN(finish);
      }
    );
  }, [phase, enter, exit]);

  const enterStyle = useAnimatedStyle(
    () => pushStyle(enter.value, width * pushOffset, reducedMotion),
    [reducedMotion, width, pushOffset]
  );
  const exitStyle = useAnimatedStyle(
    () => pushStyle(exit.value, width * pushOffset, reducedMotion),
    [reducedMotion, width, pushOffset]
  );

  /** 播放退出动画，结束后触发 onDismiss；after 用于"动作完成后再执行"的时序 */
  const close = useCallback((after?: () => void) => {
    if (closingRef.current) return;
    closingRef.current = true;
    afterRef.current = after;
    setPhase('exiting');
  }, []);

  const pageStyle = phase === 'entering' ? enterStyle : phase === 'exiting' ? exitStyle : undefined;
  return { pageStyle, settled: phase === 'settled', close };
}

/** p 为页面可见度：0 完全隐藏（右移 distance 并透明），1 在位且不透明 */
function pushStyle(p: number, distance: number, reducedMotion: boolean) {
  'worklet';
  if (reducedMotion) return { opacity: p };
  return {
    opacity: interpolate(p, [0, 0.4, 1], [0, 1, 1]),
    transform: [{ translateX: distance * (1 - p) }],
  };
}
