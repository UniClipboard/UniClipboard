import { useCallback, useEffect, useMemo, useRef } from 'react';
import * as Haptics from 'expo-haptics';
import { useAnimatedStyle, useSharedValue } from 'react-native-reanimated';
import {
  isPullToDismiss,
  PULL_TO_DISMISS_DISTANCE,
  type PullToDismissHandlers,
} from '@/utils/pullToDismiss';

/** 底部搜索框随下拉下沉的距离 */
const DOCK_PULL_SINK = 24;

/**
 * iOS 搜索层的下拉关闭:下拉时底部搜索框跟手下沉变淡,越过阈值给一次轻触感;越过阈值松手即
 * 调用 `dismiss`。搜索层自带退场动画,首页一直在它下面,所以这里不需要再等待或遮挡。
 * `handlers` 交给搜索层里的滚动视图汇报拖拽,`dockStyle` 挂到底部搜索框上。
 */
export function useSearchPullToDismiss(isSearching: boolean, dismiss: () => void) {
  const pull = useSharedValue(0);
  const lastPull = useRef(0);
  const armed = useRef(false);
  const dismissed = useRef(false);

  // 进入搜索时复位;退出时保留,让退场中的搜索框停在松手时的位置
  const reset = useCallback(() => {
    lastPull.current = 0;
    armed.current = false;
    dismissed.current = false;
    pull.value = 0;
  }, [pull]);
  useEffect(() => {
    if (isSearching) reset();
  }, [isSearching, reset]);

  const handlers = useMemo<PullToDismissHandlers>(
    () => ({
      onPull: (offsetY) => {
        if (dismissed.current) return;
        const next = Math.min(1, Math.max(0, -offsetY / PULL_TO_DISMISS_DISTANCE));
        if (next === lastPull.current) return;
        lastPull.current = next;
        pull.value = next;
        const nextArmed = next >= 1;
        if (nextArmed !== armed.current) {
          armed.current = nextArmed;
          if (nextArmed) Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light).catch(() => {});
        }
      },
      onRelease: (offsetY) => {
        if (dismissed.current || !isPullToDismiss(offsetY)) return;
        dismissed.current = true;
        dismiss();
      },
    }),
    [pull, dismiss]
  );

  const dockStyle = useAnimatedStyle(() => ({
    opacity: 1 - 0.5 * pull.value,
    transform: [{ translateY: DOCK_PULL_SINK * pull.value }],
  }));

  return { handlers, dockStyle };
}
