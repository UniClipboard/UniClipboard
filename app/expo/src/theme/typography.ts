/**
 * Typography tokens
 * 对齐 iOS HIG Text Styles
 */

import type { TextStyle } from 'react-native';

type TypeStyle = Pick<TextStyle, 'fontSize' | 'lineHeight' | 'fontWeight' | 'letterSpacing'>;

export const typography = {
  largeTitle: {
    fontSize: 34,
    lineHeight: 41,
    fontWeight: '700',
    letterSpacing: 0.37,
  },
  title1: {
    fontSize: 28,
    lineHeight: 34,
    fontWeight: '700',
    letterSpacing: 0.36,
  },
  title2: {
    fontSize: 22,
    lineHeight: 28,
    fontWeight: '700',
    letterSpacing: 0.35,
  },
  title3: {
    fontSize: 20,
    lineHeight: 25,
    fontWeight: '600',
    letterSpacing: 0.38,
  },
  headline: {
    fontSize: 17,
    lineHeight: 22,
    fontWeight: '600',
    letterSpacing: -0.43,
  },
  body: {
    fontSize: 17,
    lineHeight: 22,
    fontWeight: '400',
    letterSpacing: -0.43,
  },
  callout: {
    fontSize: 16,
    lineHeight: 21,
    fontWeight: '400',
    letterSpacing: -0.32,
  },
  subhead: {
    fontSize: 15,
    lineHeight: 20,
    fontWeight: '400',
    letterSpacing: -0.24,
  },
  footnote: {
    fontSize: 13,
    lineHeight: 18,
    fontWeight: '400',
    letterSpacing: -0.08,
  },
  caption1: {
    fontSize: 12,
    lineHeight: 16,
    fontWeight: '400',
    letterSpacing: 0,
  },
  caption2: {
    fontSize: 11,
    lineHeight: 13,
    fontWeight: '400',
    letterSpacing: 0.07,
  },
  // 区段标题(iOS 设置页那种 UPPERCASE 灰字)
  sectionHeader: {
    fontSize: 13,
    lineHeight: 18,
    fontWeight: '600',
    letterSpacing: 0.5,
  },
} as const satisfies Record<string, TypeStyle>;

export type TypographyToken = keyof typeof typography;
export type Typography = typeof typography;
