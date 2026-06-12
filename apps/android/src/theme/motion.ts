/**
 * Motion tokens
 * 时长与缓动曲线,贴近 iOS 默认动画手感
 */

import { Easing, type EasingFunction } from 'react-native';

export const duration = {
  instant: 100,
  fast: 180,
  base: 240,
  slow: 320,
  slower: 480,
} as const;

export const easing: Record<string, EasingFunction> = {
  // iOS 默认 ease-in-out 曲线
  standard: Easing.bezier(0.4, 0.0, 0.2, 1),
  // 退场:加速消失
  accelerate: Easing.bezier(0.4, 0.0, 1, 1),
  // 入场:减速到位
  decelerate: Easing.bezier(0.0, 0.0, 0.2, 1),
  // iOS spring 感觉的近似贝塞尔
  emphasized: Easing.bezier(0.2, 0.0, 0.0, 1),
};

export const motion = { duration, easing } as const;
export type Motion = typeof motion;
