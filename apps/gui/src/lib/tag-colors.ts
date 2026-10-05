import type { CSSProperties } from 'react'
import type { HistoryTagColorDto } from '@/api/daemon/history-tags'

/** A palette color (HDetail.dc.html); any other tag color is a custom `#rrggbb`. */
export type TagPresetColor = 'orange' | 'blue' | 'green' | 'purple' | 'gray'

/** The palette, in order. */
export const TAG_COLORS: readonly TagPresetColor[] = ['orange', 'blue', 'green', 'purple', 'gray']

/** The color a new tag starts with: the palette's first swatch. */
export const DEFAULT_TAG_COLOR: TagPresetColor = 'orange'

const CUSTOM_COLOR = /^#[0-9a-fA-F]{6}$/

export function isPresetColor(color: string): color is TagPresetColor {
  return (TAG_COLORS as readonly string[]).includes(color)
}

/** Whether `color` is a custom `#rrggbb` color. */
export function isCustomColor(color: string): boolean {
  return CUSTOM_COLOR.test(color)
}

// Literal class names, so Tailwind sees every one (tokens in globals.css).
const PRESET_TINTS: Record<TagPresetColor, { dot: string; bg: string; text: string }> = {
  orange: { dot: 'bg-tag-orange', bg: 'bg-tag-orange-soft', text: 'text-tag-orange-ink' },
  blue: { dot: 'bg-tag-blue', bg: 'bg-tag-blue-soft', text: 'text-tag-blue-ink' },
  green: { dot: 'bg-tag-green', bg: 'bg-tag-green-soft', text: 'text-tag-green-ink' },
  purple: { dot: 'bg-tag-purple', bg: 'bg-tag-purple-soft', text: 'text-tag-purple-ink' },
  gray: { dot: 'bg-tag-gray', bg: 'bg-tag-gray-soft', text: 'text-tag-gray-ink' },
}

/** A tag's tint: `dot` for its swatch, `chip` for a filled chip, `text` alone
 * for an outlined one. A custom color also needs `style` on the same element,
 * which sets the variables those classes read. */
export interface TagTint {
  dot: string
  chip: string
  text: string
  style?: CSSProperties
}

/** The tint of `color`; a tag without a color (or an unreadable one) is gray. */
export function tagTint(color: HistoryTagColorDto | null | undefined): TagTint {
  if (color && isCustomColor(color)) {
    // The palette's soft grounds and inks, derived from the one custom color;
    // mixing toward the foreground keeps the ink readable in both themes.
    const style = {
      '--tag': color,
      '--tag-soft': `color-mix(in oklab, ${color} 16%, transparent)`,
      '--tag-ink': `color-mix(in oklab, ${color} 70%, var(--foreground))`,
    } as CSSProperties
    return {
      dot: 'bg-(--tag)',
      chip: 'bg-(--tag-soft) text-(--tag-ink)',
      text: 'text-(--tag-ink)',
      style,
    }
  }
  const tint = PRESET_TINTS[color && isPresetColor(color) ? color : 'gray']
  return { dot: tint.dot, text: tint.text, chip: `${tint.bg} ${tint.text}` }
}
