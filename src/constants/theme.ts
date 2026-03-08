import type { ThemePreset } from '@/lib/theme-engine'
import { themePresets, DEFAULT_THEME_COLOR } from '@/lib/theme-engine'

export interface ThemeColorOption {
  name: string
  color: string
  previewDots: string[]
}

const presetList: ThemePreset[] = Object.values(themePresets)

export const THEME_COLORS: ThemeColorOption[] = presetList.map(preset => ({
  name: preset.name,
  color: preset.accentColor,
  previewDots: preset.previewDots,
}))

export { DEFAULT_THEME_COLOR }
