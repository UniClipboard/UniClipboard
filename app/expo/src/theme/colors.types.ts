/**
 * ColorScheme —— 跨平台中性语义色契约
 *
 * 这是两端(Android / iOS)唯一共享的 key 契约:
 * - `colors.android.ts` 用 Material 3 十六进制实现每个 key
 * - `colors.ios.ts` 用 PlatformColor / DynamicColorIOS 实现同一批 key
 *
 * 调用点只认这套中性名(textPrimary / accent / surfaceHigh …),不出现任何平台专有 jargon
 * (M3 的 onSurface/primary/outline,iOS 的 label/systemBackground 都藏在各自平台文件里)。
 *
 * 值类型是 `string | OpaqueColorValue`:Android 侧全是字符串,iOS 侧多为 PlatformColor 对象。
 * RN 的 color 类样式属性(color/backgroundColor/borderColor)两者都吃。若需对颜色做字符串
 * 运算(叠 alpha),必须走 colorUtils 的 alpha()/blend(),它们对 OpaqueColorValue 会安全跳过。
 */

import type { OpaqueColorValue } from 'react-native';

/** 单个颜色槽:平台字符串或 iOS 原生色对象 */
export type Color = string | OpaqueColorValue;

export interface ColorScheme {
  // === 文本 / 前景 ===
  /** 主文本(iOS label / M3 onSurface) */
  textPrimary: Color;
  /** 次要文本、占位符、次级图标(iOS secondaryLabel / M3 onSurfaceVariant) */
  textSecondary: Color;
  /** 三级文本(iOS tertiaryLabel) */
  textTertiary: Color;
  /** 禁用态文本(iOS quaternaryLabel) */
  textDisabled: Color;

  // === 强调色(品牌 / 交互主色)===
  /** 强调色:Android=M3 primary(紫),iOS=单色墨 accent */
  accent: Color;
  /** 反色表面(snackbar)上的强调色,即 M3 inversePrimary */
  inverseAccent: Color;
  /** 强调色之上的前景 */
  onAccent: Color;
  /** 强调色容器(弱化的强调背景,如次要动作按钮底) */
  accentContainer: Color;
  /** 强调色容器之上的前景 */
  onAccentContainer: Color;

  // === 表面 / 背景(由低到高的层级)===
  /** 页面级底色 */
  background: Color;
  /** 默认表面 */
  surface: Color;
  /** 最低层表面 */
  surfaceLowest: Color;
  /** 低层表面(卡片) */
  surfaceLow: Color;
  /** 中层表面 */
  surfaceMid: Color;
  /** 高层表面(浮层 / pill / sheet) */
  surfaceHigh: Color;
  /** 最高层表面 */
  surfaceHighest: Color;

  // === 线条 ===
  /** 强边框(iOS opaqueSeparator / M3 outline) */
  border: Color;
  /** 细分隔线(iOS separator / M3 outlineVariant) */
  separator: Color;

  // === 状态色 ===
  error: Color;
  onError: Color;
  errorContainer: Color;
  onErrorContainer: Color;
  errorContainerBorder: Color;
  success: Color;
  onSuccess: Color;
  successContainer: Color;
  onSuccessContainer: Color;
  successContainerBorder: Color;
  warning: Color;
  onWarning: Color;
  warningContainer: Color;
  onWarningContainer: Color;
  warningContainerBorder: Color;
  info: Color;
  onInfo: Color;
  infoContainer: Color;
  onInfoContainer: Color;
  infoContainerBorder: Color;

  // === 反色表面(snackbar 等)===
  inverseSurface: Color;
  inverseOnSurface: Color;

  // === 遮罩 / 杂项 ===
  /** 轻遮罩层 */
  overlay: Color;
  /** 模态背景遮罩 */
  backdrop: Color;
  messageSuccess: Color;
  messageError: Color;
  white: Color;
  transparent: Color;

  // === Fill 色阶(hover / pressed 状态层)===
  fillPrimary: Color;
  fillSecondary: Color;
  fillTertiary: Color;
  fillQuaternary: Color;
}
