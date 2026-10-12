import type { TagColor, TagPaletteColor } from '@/ui-kit/contract'

/** Tag palette on the GNOME palette colours. */
const paletteColor: Record<TagPaletteColor, string> = {
  orange: 'var(--orange)',
  blue: 'var(--blue)',
  green: 'var(--green)',
  purple: 'var(--purple)',
  gray: 'var(--gray)',
}

export const isCustomColor = (color: TagColor): color is `#${string}` => color.startsWith('#')

/** The solid colour of a tag's dot or square; a custom `#rrggbb` is used as it is. */
export const swatchColor = (color: TagColor): string =>
  isCustomColor(color) ? color : paletteColor[color]
