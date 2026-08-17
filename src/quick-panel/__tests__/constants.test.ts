import { describe, expect, it } from 'vitest'
import {
  clampImageCardAspectRatio,
  getQuickPanelLayoutClassNames,
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

describe('Linux quick panel layout', () => {
  it('uses an opaque unified surface with a divider instead of transparent gaps', () => {
    expect(getQuickPanelLayoutClassNames(true)).toEqual({
      container: 'flex h-screen w-screen overflow-hidden bg-background p-0',
      statusSurface: 'bg-background',
      card: 'flex h-full w-full min-w-0 flex-col overflow-hidden border border-border/50 bg-background',
      previewCard:
        'flex h-full w-full min-w-0 flex-col overflow-hidden border border-border/50 bg-card text-card-foreground',
      previewExpanded: 'flex-1 basis-0 border-l border-border/50 opacity-100 translate-x-0',
      previewReserved:
        'shrink-0 border-l border-border/50 opacity-0 translate-x-0 pointer-events-none',
    })
  })

  it('keeps the floating-card layout on non-Linux platforms', () => {
    expect(getQuickPanelLayoutClassNames(false).container).toContain('bg-transparent')
    expect(getQuickPanelLayoutClassNames(false).statusSurface).toBe('bg-transparent')
    expect(getQuickPanelLayoutClassNames(false).container).toContain('p-4')
    expect(getQuickPanelLayoutClassNames(false).card).toContain('rounded-xl')
  })
})
