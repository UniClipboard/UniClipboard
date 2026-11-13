/**
 * 颜色工具函数(平台无关,纯字符串运算)
 *
 * 注意:iOS 平台色板里的值是 PlatformColor / DynamicColorIOS 返回的 OpaqueColorValue(对象),
 * 不是十六进制字符串。alpha()/blend() 只能作用于 `#RRGGBB` 字符串,遇到对象或 rgb() 字符串
 * 直接原样返回——避免在 iOS 上对 PlatformColor 调用字符串方法而崩溃。
 */

import type { ColorValue } from 'react-native';

/** 给十六进制颜色叠加 alpha;非 `#RRGGBB` 字符串(rgb()/PlatformColor 对象)原样返回 */
export function alpha(color: ColorValue, a: number): ColorValue {
  if (typeof color !== 'string' || !color.startsWith('#') || color.length < 7) return color;
  const r = parseInt(color.slice(1, 3), 16);
  const g = parseInt(color.slice(3, 5), 16);
  const b = parseInt(color.slice(5, 7), 16);
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

/** 两个十六进制色按 opacity 混合;任一非 `#RRGGBB` 字符串则返回 base 原值 */
export function blend(base: ColorValue, overlay: ColorValue, opacity: number): ColorValue {
  if (
    typeof base !== 'string' ||
    typeof overlay !== 'string' ||
    !base.startsWith('#') ||
    !overlay.startsWith('#')
  ) {
    return base;
  }
  const br = parseInt(base.slice(1, 3), 16);
  const bg = parseInt(base.slice(3, 5), 16);
  const bb = parseInt(base.slice(5, 7), 16);
  const or = parseInt(overlay.slice(1, 3), 16);
  const og = parseInt(overlay.slice(3, 5), 16);
  const ob = parseInt(overlay.slice(5, 7), 16);
  const r = Math.round(br * (1 - opacity) + or * opacity);
  const g = Math.round(bg * (1 - opacity) + og * opacity);
  const b = Math.round(bb * (1 - opacity) + ob * opacity);
  return `rgb(${r}, ${g}, ${b})`;
}
