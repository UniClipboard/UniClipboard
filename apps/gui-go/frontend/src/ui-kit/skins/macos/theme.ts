import type { ColorScheme } from '@/ui-kit/contract'
import { isDefaultTheme, type AppliedTheme } from '@/ui-kit/theme/applied-theme'
import { alpha, mix, normalizeColors } from '@/ui-kit/theme/colors'
import { defaultThemeConfiguration } from '@/ui-kit/theme/default'

export function macTheme(scheme: ColorScheme, theme: AppliedTheme | null): Record<string, string> {
  const defaults = defaultThemeConfiguration.platforms.macos[scheme]
  if (isDefaultTheme(theme)) return defaults
  const c = normalizeColors({ ...theme!.colors })
  const fg = c.foreground,
    accent = c.primary
  return {
    ...defaults,
    '--label': fg,
    '--label-2': c.mutedForeground,
    '--label-3': alpha(fg, 0.38),
    '--label-4': alpha(fg, 0.1),
    '--window': c.background,
    '--content': c.background,
    '--alt-row': c.secondary,
    '--under-page': c.background,
    '--accent': accent,
    '--on-accent': c.primaryForeground,
    '--accent-pressed': mix(accent, c.background, 0.2),
    '--accent-tint-14': alpha(accent, 0.14),
    '--accent-tint-16': alpha(accent, 0.16),
    '--accent-tint-18': alpha(accent, 0.18),
    '--selection': c.sidebarPrimary,
    '--selection-fg': c.sidebarPrimaryForeground,
    '--selection-fg-secondary': alpha(c.sidebarPrimaryForeground, 0.8),
    '--selection-off': c.secondary,
    '--link': accent,
    '--focus': alpha(c.ring, 0.4),
    '--fill-1': c.input,
    '--fill-2': c.secondary,
    '--fill-3': c.muted,
    '--fill-4': alpha(fg, 0.03),
    '--separator': c.border,
    '--glass': c.sidebar,
    '--glass-clear': c.sidebar,
    '--glass-tinted': c.popover,
    '--popup-opaque': c.popover,
    '--popup-background': c.popover,
    '--glass-rim': c.border,
    '--ctrl': c.card,
    '--code-bg': c.background,
    '--red': c.destructive,
    '--red-tint-14': alpha(c.destructive, 0.14),
    '--red-tint-10': alpha(c.destructive, 0.1),
    '--blue-tint-10': alpha(accent, 0.1),
    '--green': c.chart2,
    '--green-tint-10': alpha(c.chart2, 0.1),
    '--orange': c.chart3,
    '--orange-tint-10': alpha(c.chart3, 0.1),
  }
}
