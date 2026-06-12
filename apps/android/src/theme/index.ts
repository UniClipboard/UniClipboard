/**
 * 主题系统
 * 导出主题配置、类型和工具函数
 */

import {
  buildScheme,
  lightColors,
  darkColors,
  PALETTES,
  DEFAULT_PALETTE_ID,
  alpha,
  blend,
  type ColorScheme,
  type PaletteId,
} from './colors';
import { spacing, type Spacing } from './spacing';
import { radius, type Radius } from './radius';
import { typography, type Typography } from './typography';
import { elevation, type Elevation } from './elevation';
import { motion, type Motion } from './motion';

export type ThemeMode = 'light' | 'dark' | 'auto';

export interface Theme {
  mode: ThemeMode;
  paletteId: PaletteId;
  colors: ColorScheme;
  isDark: boolean;
  spacing: Spacing;
  radius: Radius;
  typography: Typography;
  elevation: Elevation;
  motion: Motion;
}

/**
 * 创建主题对象
 */
export const createTheme = (
  mode: ThemeMode,
  systemColorScheme: 'light' | 'dark',
  paletteId: PaletteId = DEFAULT_PALETTE_ID
): Theme => {
  const isDark = mode === 'auto' ? systemColorScheme === 'dark' : mode === 'dark';

  return {
    mode,
    paletteId,
    colors: buildScheme(paletteId, isDark),
    isDark,
    spacing,
    radius,
    typography,
    elevation,
    motion,
  };
};

// 导出 token
export {
  lightColors,
  darkColors,
  spacing,
  radius,
  typography,
  elevation,
  motion,
  PALETTES,
  DEFAULT_PALETTE_ID,
  alpha,
  blend,
};
export type { ColorScheme, Spacing, Radius, Typography, Elevation, Motion, PaletteId };
