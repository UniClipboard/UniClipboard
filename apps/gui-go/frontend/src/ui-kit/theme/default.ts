import type { ThemeMode, ThemePreset, ThemeTokens } from '../../lib/theme-engine'
import configuration from '../../themes/default.json'

declare const __UI_PLATFORM__: 'linux' | 'macos' | 'windows'

export const defaultThemeConfiguration = configuration

function semanticColors(
  platform: keyof typeof configuration.platforms,
  mode: ThemeMode
): ThemeTokens {
  const color = (
    adw: keyof typeof configuration.platforms.linux.light,
    appkit: keyof typeof configuration.platforms.macos.light,
    fluent: keyof typeof configuration.platforms.windows.light
  ): string => {
    const value =
      platform === 'linux'
        ? configuration.platforms.linux[mode][adw]
        : platform === 'macos'
          ? configuration.platforms.macos[mode][appkit]
          : configuration.platforms.windows[mode][fluent]
    if (typeof value !== 'string' || value.trim() === '') {
      throw new Error(
        `Missing default theme color for ${platform}/${mode}: ${adw}, ${appkit}, ${fluent}`
      )
    }
    return value
  }
  const background = color('--window', '--window', 'w11-base')
  const foreground = color('--fg', '--label', 'colorNeutralForeground1')
  const primary = color('--accent', '--accent', 'colorBrandBackground')
  const primaryForeground = color('--accent-fg', '--on-accent', 'colorNeutralForegroundOnBrand')
  const secondary = color('--btn', '--fill-2', 'colorNeutralBackground1Hover')
  const border = color('--border', '--separator', 'colorNeutralStroke1')
  const sidebar = color('--sidebar', '--window', 'w11-base')
  return {
    background,
    foreground,
    card: color('--card', '--content', 'colorNeutralBackground1'),
    cardForeground: foreground,
    popover: color('--popover', '--popup-opaque', 'colorNeutralBackground1'),
    popoverForeground: foreground,
    primary,
    primaryForeground,
    secondary,
    secondaryForeground: foreground,
    muted: secondary,
    mutedForeground: color('--fg2', '--label-2', 'colorNeutralForeground2'),
    accent: color('--hover', '--fill-2', 'colorSubtleBackgroundHover'),
    accentForeground: foreground,
    destructive: color('--destructive', '--red', 'colorPaletteRedForeground1'),
    destructiveForeground: primaryForeground,
    border,
    input: border,
    ring: primary,
    chart1: primary,
    chart2: color('--green', '--green', 'w11-status-positive'),
    chart3: color('--yellow', '--yellow', 'w11-status-warning'),
    chart4: color('--purple', '--purple', 'colorPalettePurpleBorderActive'),
    chart5: color('--teal', '--pink', 'colorPaletteTealBorderActive'),
    sidebar,
    sidebarForeground: foreground,
    sidebarPrimary: primary,
    sidebarPrimaryForeground: primaryForeground,
    sidebarAccent: secondary,
    sidebarAccentForeground: foreground,
    sidebarBorder: border,
    sidebarRing: primary,
  }
}

// The build target chooses the default palette; scripts without a target use the Linux palette.
const platform = typeof __UI_PLATFORM__ === 'undefined' ? 'linux' : __UI_PLATFORM__
const light = semanticColors(platform, 'light')
const dark = semanticColors(platform, 'dark')
export const defaultTheme: ThemePreset = {
  name: configuration.name,
  accentColor: light.primary,
  previewDots: [light.primary, light.background, light.sidebar],
  light,
  dark,
}
