/**
 * Radius tokens
 * iOS 风圆角,大尺寸圆角使用 continuous curve 更贴近系统观感
 * 用法:style={{ borderRadius: radius.lg, borderCurve: 'continuous' }}
 */

export const radius = {
  none: 0,
  xs: 4,
  sm: 8,
  md: 10,
  base: 12,
  lg: 14,
  xl: 18,
  xxl: 24,
  pill: 999,
} as const;

export type RadiusToken = keyof typeof radius;
export type Radius = typeof radius;
