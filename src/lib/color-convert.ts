/**
 * Hex ↔ oklch 颜色转换工具。
 *
 * 项目主题 token 全部以 `oklch(L C H)` 字符串形式存储与应用,而 react-colorful
 * 等 picker 按惯例输出 hex（`#RRGGBB`）。本模块封装这两种格式的双向转换,
 * 边界值（白/黑/灰阶）保证 hue 落在确定值上,而不是 NaN。
 */

import { converter, parse, formatHex, type Oklch } from 'culori'

const toOklch = converter('oklch')
const toRgb = converter('rgb')

/** 把 hex（`#RRGGBB` / `#RGB`）转成 `oklch(L C H)` 字符串。 */
export function hexToOklch(hex: string): string {
  const parsed = parse(hex)
  if (!parsed) throw new Error(`Invalid hex color: ${hex}`)
  const c = toOklch(parsed) as Oklch | undefined
  if (!c) throw new Error(`Failed to convert ${hex} to oklch`)
  // hue 在纯灰阶时为 NaN（无意义），统一规范成 0,避免渲染层歧义。
  const l = c.l ?? 0
  const chroma = c.c ?? 0
  const hue = Number.isFinite(c.h) ? (c.h as number) : 0
  return `oklch(${roundTo(l, 4)} ${roundTo(chroma, 4)} ${roundTo(hue, 3)})`
}

/** 把任意 oklch 字符串（`oklch(0.5 0.2 270)` 或带百分号、deg 等）转成 `#RRGGBB`。 */
export function oklchToHex(oklch: string): string {
  const parsed = parse(oklch)
  if (!parsed) throw new Error(`Invalid oklch color: ${oklch}`)
  const rgb = toRgb(parsed)
  if (!rgb) throw new Error(`Failed to convert ${oklch} to rgb`)
  const hex = formatHex(rgb)
  if (!hex) throw new Error(`Failed to format hex from ${oklch}`)
  return hex
}

/**
 * 安全转换 oklch → hex,失败时返回 fallback。
 * Picker UI 在用户错误状态下不应崩溃,所以提供这层包装。
 */
export function oklchToHexSafe(oklch: string, fallback = '#000000'): string {
  try {
    return oklchToHex(oklch)
  } catch {
    return fallback
  }
}

function roundTo(value: number, digits: number): number {
  const factor = 10 ** digits
  return Math.round(value * factor) / factor
}
