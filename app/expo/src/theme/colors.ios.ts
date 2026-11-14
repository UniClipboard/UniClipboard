/**
 * iOS 色板 —— 原生系统语义色(PlatformColor / DynamicColorIOS)
 *
 * 实现 ColorScheme 中性契约,值全部走 iOS 原生:随浅/深色、对比度、辅助功能自动适配。
 * 命名对照(中性名 → iOS 原生):
 *   textPrimary   → label            textSecondary → secondaryLabel
 *   textTertiary  → tertiaryLabel     textDisabled  → quaternaryLabel
 *   background     → systemBackground  surface       → secondarySystemBackground
 *   surfaceLow     → secondarySystemGroupedBackground(卡片)
 *   surfaceHigh    → tertiarySystemBackground(浮层)
 *   border         → opaqueSeparator   separator     → separator
 *   accent         → 单色墨(DESIGN.md iosAccentColor)
 *   error/…        → systemRed/Green/Orange/Blue
 *
 * Metro 在 iOS 平台自动解析到本文件。深浅色由 PlatformColor / DynamicColorIOS 自适应
 * (ThemeContext 通过 Appearance.setColorScheme 把强制模式同步给系统,故此处忽略 isDark)。
 */

import { PlatformColor, DynamicColorIOS } from 'react-native';
import { iosAccent, iosAccentColor } from './iosDesignTokens';
import type { ColorScheme } from './colors.types';

export { alpha, blend } from './colorUtils';
export type { ColorScheme, Color } from './colors.types';

const dyn = (light: string, dark: string) => DynamicColorIOS({ light, dark });

/** 强调色之上的前景:与单色墨 accent 反相(浅色下 accent 深→前景浅) */
const onAccentColor = dyn(iosAccent.dark, iosAccent.light);

const IOS_SCHEME: ColorScheme = {
  // 文本 / 前景
  textPrimary: PlatformColor('label'),
  textSecondary: PlatformColor('secondaryLabel'),
  textTertiary: PlatformColor('tertiaryLabel'),
  textDisabled: PlatformColor('quaternaryLabel'),

  // 强调色(单色墨)
  accent: iosAccentColor ?? PlatformColor('label'),
  inverseAccent: onAccentColor,
  onAccent: onAccentColor,
  accentContainer: PlatformColor('secondarySystemFill'),
  onAccentContainer: PlatformColor('label'),

  // 表面 / 背景
  background: PlatformColor('systemBackground'),
  surface: PlatformColor('secondarySystemBackground'),
  surfaceLowest: PlatformColor('systemBackground'),
  surfaceLow: PlatformColor('secondarySystemGroupedBackground'),
  surfaceMid: PlatformColor('secondarySystemBackground'),
  surfaceHigh: PlatformColor('tertiarySystemBackground'),
  surfaceHighest: PlatformColor('tertiarySystemBackground'),

  // 线条
  border: PlatformColor('opaqueSeparator'),
  separator: PlatformColor('separator'),

  // 状态色(base 走系统色;container 用 DynamicColorIOS 烘焙,iOS 无等价语义)
  error: PlatformColor('systemRed'),
  onError: '#FFFFFF',
  errorContainer: dyn('#F9DEDC', '#8C1D18'),
  onErrorContainer: dyn('#410E0B', '#F9DEDC'),
  errorContainerBorder: dyn('#F2B8B5', 'rgba(242, 184, 181, 0.35)'),
  success: PlatformColor('systemGreen'),
  onSuccess: '#FFFFFF',
  successContainer: dyn('#E6F4EA', '#14331D'),
  onSuccessContainer: dyn('#1B5E20', '#A5D6A7'),
  successContainerBorder: dyn('#C8E6C9', 'rgba(48, 209, 88, 0.35)'),
  warning: PlatformColor('systemOrange'),
  onWarning: '#FFFFFF',
  warningContainer: dyn('#FFF4E5', '#3B2A0F'),
  onWarningContainer: dyn('#8C5400', '#FFD58A'),
  warningContainerBorder: dyn('#FFE0B2', 'rgba(255, 159, 10, 0.35)'),
  info: PlatformColor('systemBlue'),
  onInfo: '#FFFFFF',
  infoContainer: dyn('#E3F2FD', '#0F2A3B'),
  onInfoContainer: dyn('#0D47A1', '#90CAF9'),
  infoContainerBorder: dyn('#BBDEFB', 'rgba(100, 210, 255, 0.35)'),

  // 反色表面
  inverseSurface: dyn('#322F35', '#E6E0E9'),
  inverseOnSurface: dyn('#F5EFF7', '#322F35'),

  // 遮罩 / 杂项
  overlay: dyn('rgba(0, 0, 0, 0.3)', 'rgba(255, 255, 255, 0.1)'),
  backdrop: dyn('rgba(0, 0, 0, 0.5)', 'rgba(0, 0, 0, 0.7)'),
  messageSuccess: '#4CAF50',
  messageError: '#F44336',
  white: '#FFFFFF',
  transparent: 'transparent',

  // Fill 色阶(iOS 原生 systemFill 层级)
  fillPrimary: PlatformColor('systemFill'),
  fillSecondary: PlatformColor('secondarySystemFill'),
  fillTertiary: PlatformColor('tertiarySystemFill'),
  fillQuaternary: PlatformColor('quaternarySystemFill'),
};

// iOS 系统色自适应深浅,无需按 isDark 分支
export function buildScheme(_isDark: boolean): ColorScheme {
  return IOS_SCHEME;
}

export const lightColors = IOS_SCHEME;
export const darkColors = IOS_SCHEME;
