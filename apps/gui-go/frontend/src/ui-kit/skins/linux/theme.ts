import type { ColorScheme } from '@/ui-kit/contract'
import { isDefaultTheme, type AppliedTheme } from '@/ui-kit/theme/applied-theme'
import { alpha, mix, normalizeColors } from '@/ui-kit/theme/colors'
import { defaultThemeConfiguration } from '@/ui-kit/theme/default'

export function linuxTheme(
  scheme: ColorScheme,
  theme: AppliedTheme | null
): Record<string, string> {
  const defaults = defaultThemeConfiguration.platforms.linux[scheme]
  if (isDefaultTheme(theme)) return defaults
  const c = normalizeColors({ ...theme!.colors })
  const fg = c.foreground,
    accent = c.primary,
    red = c.destructive
  return {
    ...defaults,
    '--window': c.background,
    '--view': c.background,
    '--header': c.card,
    '--sidebar': c.sidebar,
    '--card': c.card,
    '--popover': c.popover,
    '--dialog': c.popover,
    '--fg': fg,
    '--fg2': c.mutedForeground,
    '--fg3': alpha(fg, 0.38),
    '--border': c.border,
    '--btn': c.secondary,
    '--btn-hover': mix(c.secondary, fg, 0.08),
    '--shade': alpha(fg, 0.07),
    '--hover': c.accent,
    '--press': mix(c.accent, fg, 0.12),
    '--accent': accent,
    '--accent-fg': c.primaryForeground,
    '--accent-text': accent,
    '--accent-hover': mix(accent, fg, 0.1),
    '--accent-pressed': mix(accent, c.background, 0.2),
    '--focus': alpha(c.ring, 0.5),
    '--accent-tint-10': alpha(accent, 0.1),
    '--accent-tint-14': alpha(accent, 0.14),
    '--accent-tint-18': alpha(accent, 0.18),
    '--destructive': red,
    '--destructive-hover': mix(red, fg, 0.1),
    '--destructive-pressed': mix(red, c.background, 0.2),
    '--destructive-text': red,
    '--destructive-tint-10': alpha(red, 0.1),
    '--success': c.chart2,
    '--success-text': c.chart2,
    '--success-tint-10': alpha(c.chart2, 0.12),
    '--warning': c.chart3,
    '--warning-text': c.chart3,
    '--warning-tint-10': alpha(c.chart3, 0.12),
    '--toast': c.popover,
    '--toast-fg': c.popoverForeground,
    '--code': c.background,
    '--code-ink': fg,
  }
}
