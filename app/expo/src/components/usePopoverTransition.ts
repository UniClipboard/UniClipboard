import { useCallback, useEffect, useState } from 'react';
import {
  Easing,
  useReducedMotion,
  useSharedValue,
  withSpring,
  withTiming,
  type SharedValue,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';

/**
 * 锚定浮层(FAB 菜单 / 详情操作栏的 overflow)共用的进/退场时序:
 * 打开时 spring 弹入,关闭时 150ms 淡出后再卸载。返回 `mounted`(是否挂载,让退场
 * 动画播完)与 `progress`(0→1 共享值,交给 useAnimatedStyle 驱动 opacity/scale/translate)。
 *
 * 与 AddActionsFab 用同一套曲线参数,保证两处浮层观感一致。
 * 系统开启「减弱动态效果」时跳过弹簧/淡出,progress 直接跳到终态、关闭即刻卸载。
 */
export function usePopoverTransition(open: boolean): {
  mounted: boolean;
  progress: SharedValue<number>;
} {
  const [mounted, setMounted] = useState(open);
  const progress = useSharedValue(0);
  const reducedMotion = useReducedMotion();
  const unmount = useCallback(() => setMounted(false), []);

  useEffect(() => {
    if (open) {
      setMounted(true);
      progress.value = reducedMotion
        ? 1
        : withSpring(1, { damping: 18, stiffness: 240, mass: 0.7 });
    } else if (mounted) {
      if (reducedMotion) {
        progress.value = 0;
        setMounted(false);
      } else {
        progress.value = withTiming(0, { duration: 150, easing: Easing.in(Easing.quad) }, (f) => {
          if (f) scheduleOnRN(unmount);
        });
      }
    }
    // mounted 不入依赖:开→挂载,关→播完退场再卸载
  }, [open, progress, unmount, reducedMotion]);

  return { mounted, progress };
}
