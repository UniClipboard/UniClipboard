export const DEFAULT_THEME_COLOR = 'zinc'

export type ThemeMode = 'light' | 'dark'

export interface ThemeTokens {
  background: string
  foreground: string
  card: string
  cardForeground: string
  popover: string
  popoverForeground: string
  primary: string
  primaryForeground: string
  secondary: string
  secondaryForeground: string
  muted: string
  mutedForeground: string
  accent: string
  accentForeground: string
  destructive: string
  destructiveForeground: string
  border: string
  input: string
  ring: string
  chart1: string
  chart2: string
  chart3: string
  chart4: string
  chart5: string
  sidebar: string
  sidebarForeground: string
  sidebarPrimary: string
  sidebarPrimaryForeground: string
  sidebarAccent: string
  sidebarAccentForeground: string
  sidebarBorder: string
  sidebarRing: string
}

export interface ThemePreset {
  name: string
  /**
   * Representative color used for legacy preview usage.
   * UI should prefer previewDots where available.
   */
  accentColor: string
  /**
   * 3-4 representative colors for swatch preview dots.
   */
  previewDots: string[]
  light: ThemeTokens
  dark: ThemeTokens
}

const zincLight: ThemeTokens = {
  background: 'oklch(1 0 0)',
  foreground: 'oklch(0.21 0.006 285.885)',
  card: 'oklch(1 0 0)',
  cardForeground: 'oklch(0.21 0.006 285.885)',
  popover: 'oklch(1 0 0)',
  popoverForeground: 'oklch(0.21 0.006 285.885)',
  primary: 'oklch(0.21 0.006 285.885)',
  primaryForeground: 'oklch(0.985 0 0)',
  secondary: 'oklch(0.961 0.006 286.478)',
  secondaryForeground: 'oklch(0.21 0.006 285.885)',
  muted: 'oklch(0.961 0.006 286.478)',
  mutedForeground: 'oklch(0.552 0.014 285.938)',
  accent: 'oklch(0.961 0.006 286.478)',
  accentForeground: 'oklch(0.21 0.006 285.885)',
  destructive: 'oklch(0.577 0.245 27.325)',
  destructiveForeground: 'oklch(0.985 0 0)',
  border: 'oklch(0.897 0.01 285.444)',
  input: 'oklch(0.897 0.01 285.444)',
  ring: 'oklch(0.21 0.006 285.885)',
  chart1: 'oklch(0.809 0.105 251.813)',
  chart2: 'oklch(0.623 0.214 259.815)',
  chart3: 'oklch(0.546 0.245 262.881)',
  chart4: 'oklch(0.488 0.243 264.376)',
  chart5: 'oklch(0.424 0.199 265.638)',
  sidebar: 'oklch(0.96 0.006 286.478)',
  sidebarForeground: 'oklch(0.21 0.006 285.885)',
  sidebarPrimary: 'oklch(0.21 0.006 285.885)',
  sidebarPrimaryForeground: 'oklch(0.985 0 0)',
  sidebarAccent: 'oklch(0.961 0.006 286.478)',
  sidebarAccentForeground: 'oklch(0.21 0.006 285.885)',
  sidebarBorder: 'oklch(0.89 0.01 285.444)',
  sidebarRing: 'oklch(0.87 0.01 285.444)',
}

const zincDark: ThemeTokens = {
  background: 'oklch(0.21 0.006 285.885)',
  foreground: 'oklch(0.985 0 0)',
  card: 'oklch(0.21 0.006 285.885)',
  cardForeground: 'oklch(0.985 0 0)',
  popover: 'oklch(0.21 0.006 285.885)',
  popoverForeground: 'oklch(0.985 0 0)',
  primary: 'oklch(0.985 0 0)',
  primaryForeground: 'oklch(0.21 0.006 285.885)',
  secondary: 'oklch(0.274 0.006 286.033)',
  secondaryForeground: 'oklch(0.985 0 0)',
  muted: 'oklch(0.274 0.006 286.033)',
  mutedForeground: 'oklch(0.705 0.015 286.067)',
  accent: 'oklch(0.274 0.006 286.033)',
  accentForeground: 'oklch(0.985 0 0)',
  destructive: 'oklch(0.396 0.141 25.723)',
  destructiveForeground: 'oklch(0.985 0 0)',
  border: 'oklch(0.274 0.006 286.033)',
  input: 'oklch(0.274 0.006 286.033)',
  ring: 'oklch(0.83 0.01 285.444)',
  chart1: 'oklch(0.809 0.105 251.813)',
  chart2: 'oklch(0.623 0.214 259.815)',
  chart3: 'oklch(0.546 0.245 262.881)',
  chart4: 'oklch(0.488 0.243 264.376)',
  chart5: 'oklch(0.424 0.199 265.638)',
  sidebar: 'oklch(0.24 0.006 286.033)',
  sidebarForeground: 'oklch(0.985 0 0)',
  sidebarPrimary: 'oklch(0.985 0 0)',
  sidebarPrimaryForeground: 'oklch(0.21 0.006 285.885)',
  sidebarAccent: 'oklch(0.274 0.006 286.033)',
  sidebarAccentForeground: 'oklch(0.985 0 0)',
  sidebarBorder: 'oklch(0.19 0.006 286.033)',
  sidebarRing: 'oklch(0.83 0.01 285.444)',
}

// NOTE: The remaining presets are curated from the existing CSS theme files.
// They are intentionally limited to color-related tokens to keep the engine focused.

const catppuccinLight: ThemeTokens = {
  background: 'oklch(0.9578 0.0058 264.5321)',
  foreground: 'oklch(0.4355 0.043 279.325)',
  card: 'oklch(1 0 0)',
  cardForeground: 'oklch(0.4355 0.043 279.325)',
  popover: 'oklch(0.8575 0.0145 268.4756)',
  popoverForeground: 'oklch(0.4355 0.043 279.325)',
  primary: 'oklch(0.5547 0.2503 297.0156)',
  primaryForeground: 'oklch(1 0 0)',
  secondary: 'oklch(0.8575 0.0145 268.4756)',
  secondaryForeground: 'oklch(0.4355 0.043 279.325)',
  muted: 'oklch(0.906 0.0117 264.5071)',
  mutedForeground: 'oklch(0.5471 0.0343 279.0837)',
  accent: 'oklch(0.682 0.1448 235.3822)',
  accentForeground: 'oklch(1 0 0)',
  destructive: 'oklch(0.5505 0.2155 19.8095)',
  destructiveForeground: 'oklch(1 0 0)',
  border: 'oklch(0.8083 0.0174 271.1982)',
  input: 'oklch(0.8575 0.0145 268.4756)',
  ring: 'oklch(0.5547 0.2503 297.0156)',
  chart1: 'oklch(0.5547 0.2503 297.0156)',
  chart2: 'oklch(0.682 0.1448 235.3822)',
  chart3: 'oklch(0.625 0.1772 140.4448)',
  chart4: 'oklch(0.692 0.2041 42.4293)',
  chart5: 'oklch(0.7141 0.1045 33.0967)',
  sidebar: 'oklch(0.9335 0.0087 264.5206)',
  sidebarForeground: 'oklch(0.4355 0.043 279.325)',
  sidebarPrimary: 'oklch(0.5547 0.2503 297.0156)',
  sidebarPrimaryForeground: 'oklch(1 0 0)',
  sidebarAccent: 'oklch(0.682 0.1448 235.3822)',
  sidebarAccentForeground: 'oklch(1 0 0)',
  sidebarBorder: 'oklch(0.8083 0.0174 271.1982)',
  sidebarRing: 'oklch(0.5547 0.2503 297.0156)',
}

const catppuccinDark: ThemeTokens = {
  background: 'oklch(0.2155 0.0254 284.0647)',
  foreground: 'oklch(0.8787 0.0426 272.2767)',
  card: 'oklch(0.2429 0.0304 283.911)',
  cardForeground: 'oklch(0.8787 0.0426 272.2767)',
  popover: 'oklch(0.4037 0.032 280.152)',
  popoverForeground: 'oklch(0.8787 0.0426 272.2767)',
  primary: 'oklch(0.7871 0.1187 304.7693)',
  primaryForeground: 'oklch(0.2429 0.0304 283.911)',
  secondary: 'oklch(0.4765 0.034 278.643)',
  secondaryForeground: 'oklch(0.8787 0.0426 272.2767)',
  muted: 'oklch(0.2973 0.0294 276.2144)',
  mutedForeground: 'oklch(0.751 0.0396 273.932)',
  accent: 'oklch(0.8467 0.0833 210.2545)',
  accentForeground: 'oklch(0.2429 0.0304 283.911)',
  destructive: 'oklch(0.7556 0.1297 2.7642)',
  destructiveForeground: 'oklch(0.2429 0.0304 283.911)',
  border: 'oklch(0.324 0.0319 281.9784)',
  input: 'oklch(0.324 0.0319 281.9784)',
  ring: 'oklch(0.7871 0.1187 304.7693)',
  chart1: 'oklch(0.7871 0.1187 304.7693)',
  chart2: 'oklch(0.8467 0.0833 210.2545)',
  chart3: 'oklch(0.8577 0.1092 142.7153)',
  chart4: 'oklch(0.8237 0.1015 52.6294)',
  chart5: 'oklch(0.9226 0.0238 30.4919)',
  sidebar: 'oklch(0.1828 0.0204 284.2039)',
  sidebarForeground: 'oklch(0.8787 0.0426 272.2767)',
  sidebarPrimary: 'oklch(0.7871 0.1187 304.7693)',
  sidebarPrimaryForeground: 'oklch(0.2429 0.0304 283.911)',
  sidebarAccent: 'oklch(0.8467 0.0833 210.2545)',
  sidebarAccentForeground: 'oklch(0.2429 0.0304 283.911)',
  sidebarBorder: 'oklch(0.4037 0.032 280.152)',
  sidebarRing: 'oklch(0.7871 0.1187 304.7693)',
}

const t3chatLight: ThemeTokens = {
  background: 'oklch(0.9754 0.0084 325.6414)',
  foreground: 'oklch(0.3257 0.1161 325.0372)',
  card: 'oklch(0.9754 0.0084 325.6414)',
  cardForeground: 'oklch(0.3257 0.1161 325.0372)',
  popover: 'oklch(1 0 0)',
  popoverForeground: 'oklch(0.3257 0.1161 325.0372)',
  primary: 'oklch(0.5316 0.1409 355.1999)',
  primaryForeground: 'oklch(1 0 0)',
  secondary: 'oklch(0.8696 0.0675 334.8991)',
  secondaryForeground: 'oklch(0.4448 0.1341 324.7991)',
  muted: 'oklch(0.9395 0.026 331.5454)',
  mutedForeground: 'oklch(0.4924 0.1244 324.4523)',
  accent: 'oklch(0.8696 0.0675 334.8991)',
  accentForeground: 'oklch(0.4448 0.1341 324.7991)',
  destructive: 'oklch(0.5248 0.1368 20.8317)',
  destructiveForeground: 'oklch(1 0 0)',
  border: 'oklch(0.8568 0.0829 328.911)',
  input: 'oklch(0.8517 0.0558 336.6002)',
  ring: 'oklch(0.5916 0.218 0.5844)',
  chart1: 'oklch(0.6038 0.2363 344.4657)',
  chart2: 'oklch(0.4445 0.2251 300.6246)',
  chart3: 'oklch(0.379 0.0438 226.1538)',
  chart4: 'oklch(0.833 0.1185 88.3461)',
  chart5: 'oklch(0.7843 0.1256 58.9964)',
  sidebar: 'oklch(0.936 0.0288 320.5788)',
  sidebarForeground: 'oklch(0.4948 0.1909 354.5435)',
  sidebarPrimary: 'oklch(0.3963 0.0251 285.1962)',
  sidebarPrimaryForeground: 'oklch(0.9668 0.0124 337.5228)',
  sidebarAccent: 'oklch(0.9789 0.0013 106.4235)',
  sidebarAccentForeground: 'oklch(0.3963 0.0251 285.1962)',
  sidebarBorder: 'oklch(0.9383 0.0026 48.7178)',
  sidebarRing: 'oklch(0.5916 0.218 0.5844)',
}

const t3chatDark: ThemeTokens = {
  background: 'oklch(0.2409 0.0201 307.5346)',
  foreground: 'oklch(0.8398 0.0387 309.5391)',
  card: 'oklch(0.2803 0.0232 307.5413)',
  cardForeground: 'oklch(0.8456 0.0302 341.4597)',
  popover: 'oklch(0.1548 0.0132 338.9015)',
  popoverForeground: 'oklch(0.9647 0.0091 341.8035)',
  primary: 'oklch(0.4607 0.1853 4.0994)',
  primaryForeground: 'oklch(0.856 0.0618 346.3684)',
  secondary: 'oklch(0.3137 0.0306 310.061)',
  secondaryForeground: 'oklch(0.8483 0.0382 307.9613)',
  muted: 'oklch(0.2634 0.0219 309.4748)',
  mutedForeground: 'oklch(0.794 0.0372 307.1032)',
  accent: 'oklch(0.3649 0.0508 308.4911)',
  accentForeground: 'oklch(0.9647 0.0091 341.8035)',
  destructive: 'oklch(0.2258 0.0524 12.6119)',
  destructiveForeground: 'oklch(1 0 0)',
  border: 'oklch(0.3286 0.0154 343.4461)',
  input: 'oklch(0.3387 0.0195 332.8347)',
  ring: 'oklch(0.5916 0.218 0.5844)',
  chart1: 'oklch(0.5316 0.1409 355.1999)',
  chart2: 'oklch(0.5633 0.1912 306.8561)',
  chart3: 'oklch(0.7227 0.1502 60.5799)',
  chart4: 'oklch(0.6193 0.2029 312.7422)',
  chart5: 'oklch(0.6118 0.2093 6.1387)',
  sidebar: 'oklch(0.1893 0.0163 331.0475)',
  sidebarForeground: 'oklch(0.8607 0.0293 343.6612)',
  sidebarPrimary: 'oklch(0.4882 0.2172 264.3763)',
  sidebarPrimaryForeground: 'oklch(1 0 0)',
  sidebarAccent: 'oklch(0.2337 0.0261 338.1961)',
  sidebarAccentForeground: 'oklch(0.9674 0.0013 286.3752)',
  sidebarBorder: 'oklch(0 0 0)',
  sidebarRing: 'oklch(0.5916 0.218 0.5844)',
}

const claudeLight: ThemeTokens = {
  background: 'oklch(0.9818 0.0054 95.0986)',
  foreground: 'oklch(0.3438 0.0269 95.7226)',
  card: 'oklch(0.9818 0.0054 95.0986)',
  cardForeground: 'oklch(0.1908 0.002 106.5859)',
  popover: 'oklch(1 0 0)',
  popoverForeground: 'oklch(0.2671 0.0196 98.939)',
  primary: 'oklch(0.6171 0.1375 39.0427)',
  primaryForeground: 'oklch(1 0 0)',
  secondary: 'oklch(0.9245 0.0138 92.9892)',
  secondaryForeground: 'oklch(0.4334 0.0177 98.6048)',
  muted: 'oklch(0.9341 0.0153 90.239)',
  mutedForeground: 'oklch(0.6059 0.0075 97.4233)',
  accent: 'oklch(0.9245 0.0138 92.9892)',
  accentForeground: 'oklch(0.2671 0.0196 98.939)',
  destructive: 'oklch(0.1908 0.002 106.5859)',
  destructiveForeground: 'oklch(1 0 0)',
  border: 'oklch(0.8847 0.0069 97.3627)',
  input: 'oklch(0.7621 0.0156 98.3528)',
  ring: 'oklch(0.6171 0.1375 39.0427)',
  chart1: 'oklch(0.5583 0.1276 42.9956)',
  chart2: 'oklch(0.6898 0.1581 290.4107)',
  chart3: 'oklch(0.8816 0.0276 93.128)',
  chart4: 'oklch(0.8822 0.0403 298.1792)',
  chart5: 'oklch(0.5608 0.1348 42.0584)',
  sidebar: 'oklch(0.9663 0.008 98.8792)',
  sidebarForeground: 'oklch(0.359 0.0051 106.6524)',
  sidebarPrimary: 'oklch(0.6171 0.1375 39.0427)',
  sidebarPrimaryForeground: 'oklch(0.9881 0 0)',
  sidebarAccent: 'oklch(0.9245 0.0138 92.9892)',
  sidebarAccentForeground: 'oklch(0.325 0 0)',
  sidebarBorder: 'oklch(0.9401 0 0)',
  sidebarRing: 'oklch(0.7731 0 0)',
}

const claudeDark: ThemeTokens = {
  background: 'oklch(0.2679 0.0036 106.6427)',
  foreground: 'oklch(0.8074 0.0142 93.0137)',
  card: 'oklch(0.2679 0.0036 106.6427)',
  cardForeground: 'oklch(0.9818 0.0054 95.0986)',
  popover: 'oklch(0.3085 0.0035 106.6039)',
  popoverForeground: 'oklch(0.9211 0.004 106.4781)',
  primary: 'oklch(0.6724 0.1308 38.7559)',
  primaryForeground: 'oklch(1 0 0)',
  secondary: 'oklch(0.9818 0.0054 95.0986)',
  secondaryForeground: 'oklch(0.3085 0.0035 106.6039)',
  muted: 'oklch(0.2213 0.0038 106.707)',
  mutedForeground: 'oklch(0.7713 0.0169 99.0657)',
  accent: 'oklch(0.213 0.0078 95.4245)',
  accentForeground: 'oklch(0.9663 0.008 98.8792)',
  destructive: 'oklch(0.6368 0.2078 25.3313)',
  destructiveForeground: 'oklch(1 0 0)',
  border: 'oklch(0.3618 0.0101 106.8928)',
  input: 'oklch(0.4336 0.0113 100.2195)',
  ring: 'oklch(0.6724 0.1308 38.7559)',
  chart1: 'oklch(0.5583 0.1276 42.9956)',
  chart2: 'oklch(0.6898 0.1581 290.4107)',
  chart3: 'oklch(0.213 0.0078 95.4245)',
  chart4: 'oklch(0.3074 0.0516 289.323)',
  chart5: 'oklch(0.5608 0.1348 42.0584)',
  sidebar: 'oklch(0.2357 0.0024 67.7077)',
  sidebarForeground: 'oklch(0.8074 0.0142 93.0137)',
  sidebarPrimary: 'oklch(0.325 0 0)',
  sidebarPrimaryForeground: 'oklch(0.9881 0 0)',
  sidebarAccent: 'oklch(0.168 0.002 106.6177)',
  sidebarAccentForeground: 'oklch(0.8074 0.0142 93.0137)',
  sidebarBorder: 'oklch(0.9401 0 0)',
  sidebarRing: 'oklch(0.7731 0 0)',
}

const PRESETS: Record<string, ThemePreset> = {
  zinc: {
    name: 'zinc',
    accentColor: '#52525b',
    previewDots: [zincLight.primary, zincLight.accent, zincLight.secondary, zincLight.border],
    light: zincLight,
    dark: zincDark,
  },
  catppuccin: {
    name: 'catppuccin',
    accentColor: '#cba6f7',
    previewDots: [
      catppuccinLight.primary,
      catppuccinLight.accent,
      catppuccinLight.secondary,
      catppuccinLight.border,
    ],
    light: catppuccinLight,
    dark: catppuccinDark,
  },
  t3chat: {
    name: 't3chat',
    accentColor: '#a3004c',
    previewDots: [
      t3chatLight.primary,
      t3chatLight.accent,
      t3chatLight.secondary,
      t3chatLight.border,
    ],
    light: t3chatLight,
    dark: t3chatDark,
  },
  claude: {
    name: 'claude',
    accentColor: '#d97757',
    previewDots: [
      claudeLight.primary,
      claudeLight.accent,
      claudeLight.secondary,
      claudeLight.border,
    ],
    light: claudeLight,
    dark: claudeDark,
  },
}

function getPresetOrDefault(themeName: string | null | undefined): ThemePreset {
  if (!themeName) return PRESETS[DEFAULT_THEME_COLOR]

  const preset = PRESETS[themeName]
  if (!preset) {
    return PRESETS[DEFAULT_THEME_COLOR]
  }

  return preset
}

function getTokens(themeName: string | null | undefined, mode: ThemeMode): ThemeTokens {
  const preset = getPresetOrDefault(themeName)
  return mode === 'dark' ? preset.dark : preset.light
}

export function applyThemePreset(
  themeName: string | null | undefined,
  mode: ThemeMode,
  root: HTMLElement
): void {
  const tokens = getTokens(themeName, mode)
  const preset = getPresetOrDefault(themeName)

  const setVar = (name: string, value: string) => {
    root.style.setProperty(name, value)
  }

  setVar('--background', tokens.background)
  setVar('--foreground', tokens.foreground)
  setVar('--card', tokens.card)
  setVar('--card-foreground', tokens.cardForeground)
  setVar('--popover', tokens.popover)
  setVar('--popover-foreground', tokens.popoverForeground)
  setVar('--primary', tokens.primary)
  setVar('--primary-foreground', tokens.primaryForeground)
  setVar('--secondary', tokens.secondary)
  setVar('--secondary-foreground', tokens.secondaryForeground)
  setVar('--muted', tokens.muted)
  setVar('--muted-foreground', tokens.mutedForeground)
  setVar('--accent', tokens.accent)
  setVar('--accent-foreground', tokens.accentForeground)
  setVar('--destructive', tokens.destructive)
  setVar('--destructive-foreground', tokens.destructiveForeground)
  setVar('--border', tokens.border)
  setVar('--input', tokens.input)
  setVar('--ring', tokens.ring)
  setVar('--chart-1', tokens.chart1)
  setVar('--chart-2', tokens.chart2)
  setVar('--chart-3', tokens.chart3)
  setVar('--chart-4', tokens.chart4)
  setVar('--chart-5', tokens.chart5)
  setVar('--sidebar', tokens.sidebar)
  setVar('--sidebar-foreground', tokens.sidebarForeground)
  setVar('--sidebar-primary', tokens.sidebarPrimary)
  setVar('--sidebar-primary-foreground', tokens.sidebarPrimaryForeground)
  setVar('--sidebar-accent', tokens.sidebarAccent)
  setVar('--sidebar-accent-foreground', tokens.sidebarAccentForeground)
  setVar('--sidebar-border', tokens.sidebarBorder)
  setVar('--sidebar-ring', tokens.sidebarRing)

  // Keep data-theme in sync for any CSS selectors that still rely on it.
  root.setAttribute('data-theme', preset.name)
}

export function getThemePreviewDots(
  themeName: string | null | undefined,
  mode: ThemeMode
): string[] {
  const preset = getPresetOrDefault(themeName)
  const dots = preset.previewDots

  // Ensure we always return 3-4 items for UI simplicity.
  if (dots.length >= 3 && dots.length <= 4) {
    return dots
  }

  if (dots.length > 4) {
    return dots.slice(0, 4)
  }

  if (dots.length === 2) {
    return [...dots, dots[1]]
  }

  if (dots.length === 1) {
    return [dots[0], dots[0], dots[0]]
  }

  // As a last resort, derive from tokens.
  const tokens = getTokens(themeName, mode)
  return [tokens.primary, tokens.accent, tokens.secondary]
}

export const themePresets = PRESETS
