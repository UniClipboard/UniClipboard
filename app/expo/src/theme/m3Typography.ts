/**
 * Material 3 type scale(Android 专用)
 *
 * typography.ts 按 iOS HIG 命名与取值;Android 渲染路径应改用本表,
 * 与 Compose MaterialTheme.typography 保持同一套字阶(字号 / 行高 / 字重 / 字距)。
 * https://m3.material.io/styles/typography/type-scale-tokens
 */

import type { TextStyle } from 'react-native';

type TypeStyle = Pick<TextStyle, 'fontSize' | 'lineHeight' | 'fontWeight' | 'letterSpacing'>;

export const m3Type = {
  headlineSmall: { fontSize: 24, lineHeight: 32, fontWeight: '400', letterSpacing: 0 },
  titleLarge: { fontSize: 22, lineHeight: 28, fontWeight: '400', letterSpacing: 0 },
  titleMedium: { fontSize: 16, lineHeight: 24, fontWeight: '500', letterSpacing: 0.15 },
  titleSmall: { fontSize: 14, lineHeight: 20, fontWeight: '500', letterSpacing: 0.1 },
  bodyLarge: { fontSize: 16, lineHeight: 24, fontWeight: '400', letterSpacing: 0.5 },
  bodyMedium: { fontSize: 14, lineHeight: 20, fontWeight: '400', letterSpacing: 0.25 },
  bodySmall: { fontSize: 12, lineHeight: 16, fontWeight: '400', letterSpacing: 0.4 },
  labelLarge: { fontSize: 14, lineHeight: 20, fontWeight: '500', letterSpacing: 0.1 },
  labelMedium: { fontSize: 12, lineHeight: 16, fontWeight: '500', letterSpacing: 0.5 },
  labelSmall: { fontSize: 11, lineHeight: 16, fontWeight: '500', letterSpacing: 0.5 },
} as const satisfies Record<string, TypeStyle>;

/** M3 最小触控目标(dp) */
export const M3_MIN_TOUCH_TARGET = 48;

export type M3TypeToken = keyof typeof m3Type;
