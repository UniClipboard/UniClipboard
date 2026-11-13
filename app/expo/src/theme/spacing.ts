/**
 * Spacing tokens
 * 基于 4pt 网格,覆盖 iOS HIG 常用层级
 */

export const spacing = {
  none: 0,
  xxs: 2,
  xs: 4,
  sm: 8,
  md: 12,
  base: 16,
  lg: 20,
  xl: 24,
  xxl: 32,
  xxxl: 40,
  huge: 56,
} as const;

export type SpacingToken = keyof typeof spacing;
export type Spacing = typeof spacing;
