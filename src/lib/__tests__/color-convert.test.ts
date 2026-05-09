import { describe, expect, it } from 'vitest'
import { hexToOklch, oklchToHex, oklchToHexSafe } from '../color-convert'

describe('hexToOklch', () => {
  it('converts a typical hex to oklch with finite hue', () => {
    const oklch = hexToOklch('#3b82f6')
    expect(oklch).toMatch(/^oklch\([\d.]+ [\d.]+ [\d.]+\)$/)
  })

  it('handles 3-digit hex', () => {
    const oklch = hexToOklch('#fff')
    expect(oklch).toMatch(/^oklch\([\d.]+ [\d.]+ [\d.]+\)$/)
  })

  it('returns hue=0 for greyscale (NaN hue normalised)', () => {
    const white = hexToOklch('#ffffff')
    const black = hexToOklch('#000000')
    const grey = hexToOklch('#808080')
    // 灰阶 hue 在 oklch 上是 NaN, 规范化后必须落在 0。
    expect(white).toMatch(/ 0\)$/)
    expect(black).toMatch(/ 0\)$/)
    expect(grey).toMatch(/ 0\)$/)
  })

  it('throws on invalid hex', () => {
    expect(() => hexToOklch('not-a-color')).toThrow()
  })
})

describe('oklchToHex', () => {
  it('round-trips a typical color within 1 unit per channel', () => {
    const original = '#3b82f6'
    const oklch = hexToOklch(original)
    const back = oklchToHex(oklch)
    // OKLCH/sRGB 浮点损失允许每通道 ±1。
    expect(back.length).toBe(7)
    const r = parseInt(back.slice(1, 3), 16)
    const g = parseInt(back.slice(3, 5), 16)
    const b = parseInt(back.slice(5, 7), 16)
    expect(Math.abs(r - 0x3b)).toBeLessThanOrEqual(1)
    expect(Math.abs(g - 0x82)).toBeLessThanOrEqual(1)
    expect(Math.abs(b - 0xf6)).toBeLessThanOrEqual(1)
  })

  it('parses values with decimal hue', () => {
    const hex = oklchToHex('oklch(0.21 0.006 285.885)')
    expect(hex).toMatch(/^#[0-9a-f]{6}$/)
  })

  it('throws on invalid oklch', () => {
    expect(() => oklchToHex('not-a-color')).toThrow()
  })
})

describe('oklchToHexSafe', () => {
  it('returns fallback on invalid input', () => {
    expect(oklchToHexSafe('garbage', '#abcdef')).toBe('#abcdef')
  })

  it('returns conversion on valid input', () => {
    const hex = oklchToHexSafe('oklch(0.21 0.006 285.885)')
    expect(hex).toMatch(/^#[0-9a-f]{6}$/)
  })
})
