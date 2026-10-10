import { describe, expect, it } from 'vitest'
import { isCustomColor, isPresetColor, tagTint } from '@/lib/tag-colors'

describe('tagTint', () => {
  it('uses the palette tokens for a palette color, and gray for none', () => {
    expect(tagTint('orange')).toEqual({
      dot: 'bg-tag-orange',
      chip: 'bg-tag-orange-soft text-tag-orange-ink',
      text: 'text-tag-orange-ink',
    })
    expect(tagTint(undefined).dot).toBe('bg-tag-gray')
    expect(tagTint('red').dot).toBe('bg-tag-gray')
  })

  it('derives a custom color’s ground and ink from the color itself', () => {
    const tint = tagTint('#3e5fa8')
    expect(tint.dot).toBe('bg-(--tag)')
    expect(tint.style).toMatchObject({
      '--tag': '#3e5fa8',
      '--tag-soft': 'color-mix(in oklab, #3e5fa8 16%, transparent)',
    })
  })

  it('tells palette names from #rrggbb colors', () => {
    expect(isPresetColor('purple')).toBe(true)
    expect(isPresetColor('#7a3e8f')).toBe(false)
    expect(isCustomColor('#7A3E8F')).toBe(true)
    expect(isCustomColor('#7a3')).toBe(false)
  })
})
