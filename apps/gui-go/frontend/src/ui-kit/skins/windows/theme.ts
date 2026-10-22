import type { Theme } from '@fluentui/react-components'
import type { TagColor, TagPaletteColor } from '@/ui-kit/contract'
import type { ColorScheme } from '@/ui-kit/contract'
import { isDefaultTheme, type AppliedTheme } from '@/ui-kit/theme/applied-theme'
import { alpha, mix, normalizeColors } from '@/ui-kit/theme/colors'
import { defaultThemeConfiguration } from '@/ui-kit/theme/default'

/** Solid colours for the dots and squares of smart views and tags; readable on both schemes. */
const paletteSwatches: Record<TagPaletteColor, string> = {
  orange: '#E0703A',
  blue: '#3A7BD5',
  gray: '#8A8A8A',
  purple: '#A855C8',
  green: '#2E9E5B',
}

/** A custom `#rrggbb` tag colour is its own swatch. */
export const swatch = (color: TagColor): string =>
  color.startsWith('#') ? color : paletteSwatches[color as TagPaletteColor]

// The Windows appearance is a projection of the same palette used by the other skins.

export function windowsTheme(scheme: ColorScheme, theme: AppliedTheme | null): Theme {
  const defaults = defaultThemeConfiguration.platforms.windows[scheme] as Theme
  if (isDefaultTheme(theme)) return defaults
  const c = normalizeColors({ ...theme!.colors })
  const output: Theme & Record<string, string | number> = { ...defaults }
  const hover = mix(c.primary, c.foreground, 0.1)
  const pressed = mix(c.primary, c.background, 0.2)
  // Fluent has multiple token families for buttons, switches, checkboxes, links and focus rings.
  for (const key of Object.keys(output)) {
    if (/^color(?:Compound)?Brand/.test(key)) {
      output[key] = /Hover$/.test(key) ? hover : /Pressed$/.test(key) ? pressed : c.primary
      if (/Background2/.test(key)) output[key] = c.accent
    } else if (/^colorNeutralBackground/.test(key)) {
      output[key] = /Hover/.test(key)
        ? c.accent
        : /Pressed|Selected/.test(key)
          ? c.secondary
          : c.card
    } else if (/^colorNeutralForeground/.test(key)) {
      output[key] = /Disabled/.test(key)
        ? alpha(c.foreground, 0.38)
        : /[234]/.test(key)
          ? c.mutedForeground
          : c.foreground
    } else if (/^colorNeutralStroke/.test(key)) {
      output[key] = /Focus/.test(key) ? c.ring : c.border
    }
  }
  return {
    ...output,
    colorNeutralForegroundOnBrand: c.primaryForeground,
    colorNeutralForegroundInverted: c.primaryForeground,
    colorNeutralBackgroundInverted: c.primary,
    colorStrokeFocus1: c.background,
    colorStrokeFocus2: c.ring,
    colorNeutralBackground1: c.popover,
    colorNeutralBackground2: c.background,
    colorSubtleBackgroundHover: c.accent,
    colorSubtleBackgroundPressed: c.secondary,
    colorSubtleBackgroundSelected: c.accent,
    colorPaletteRedForeground1: c.destructive,
    colorPaletteRedBackground1: alpha(c.destructive, 0.12),
    colorPaletteGreenForeground1: c.chart2,
    colorPaletteYellowForeground1: c.chart3,
    'w11-base': c.background,
    'w11-card': c.card,
    'w11-card-solid': c.card,
    'w11-acrylic': c.popover,
    'w11-detail': c.background,
    'w11-selected': c.accent,
    'w11-card-stroke': c.border,
    'w11-divider': c.border,
    'w11-stroke-elevation': c.border,
    'w11-code-bg': c.background,
    'w11-notice-info': c.muted,
    'w11-notice-success': alpha(c.chart2, 0.12),
    'w11-notice-warning': alpha(c.chart3, 0.12),
    'w11-notice-error': alpha(c.destructive, 0.12),
    'w11-status-info': c.primary,
    'w11-status-positive': c.chart2,
    'w11-status-warning': c.chart3,
    'w11-status-critical': c.destructive,
    'w11-status-neutral': c.mutedForeground,
  } as Theme
}
