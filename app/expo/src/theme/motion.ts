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

/**
 * 锚定式浮层（卡片上下文菜单、分词选择）共用的进出场参数。
 * overshootClamping：保留弹簧的减速手感，但到位即停不回弹——
 * 浮层一旦就位用户就开始读内容了，任何过冲缩放都是干扰。
 */
export const overlayMotion = {
  enterSpring: { damping: 30, stiffness: 380, overshootClamping: true },
  exitDuration: 160,
  reducedMotionDuration: 160,
} as const;

export const motion = { duration, easing } as const;
export type Motion = typeof motion;
