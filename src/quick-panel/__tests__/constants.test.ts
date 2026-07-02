import { describe, expect, it } from 'vitest'
import {
  clampImageCardAspectRatio,
  IMAGE_CARD_MAX_ASPECT_RATIO,
  IMAGE_CARD_MIN_ASPECT_RATIO,
} from '@/quick-panel/constants'

describe('clampImageCardAspectRatio', () => {
  it('defaults to 1 (square) when aspectRatio is undefined', () => {
    expect(clampImageCardAspectRatio(undefined)).toBe(1)
  })

  it('defaults to 1 for non-finite or non-positive values', () => {
    expect(clampImageCardAspectRatio(Number.NaN)).toBe(1)
    expect(clampImageCardAspectRatio(Number.POSITIVE_INFINITY)).toBe(1)
    expect(clampImageCardAspectRatio(0)).toBe(1)
    expect(clampImageCardAspectRatio(-2)).toBe(1)
  })

  it('passes through values inside the clamp range unchanged', () => {
    expect(clampImageCardAspectRatio(1.5)).toBe(1.5)
  })

  it('clamps extreme portrait ratios to the minimum', () => {
    expect(clampImageCardAspectRatio(0.1)).toBe(IMAGE_CARD_MIN_ASPECT_RATIO)
  })

  it('clamps extreme landscape ratios to the maximum', () => {
    expect(clampImageCardAspectRatio(10)).toBe(IMAGE_CARD_MAX_ASPECT_RATIO)
  })
})
