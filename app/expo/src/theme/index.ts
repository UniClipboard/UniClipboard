/**
 * 主题系统
 * 导出主题配置、类型和工具函数
 */

import { buildScheme, lightColors, darkColors, alpha, blend, type ColorScheme } from './colors';
import { spacing, type Spacing } from './spacing';
import { radius, type Radius } from './radius';
import { typography, type Typography } from './typography';
import { elevation, type Elevation } from './elevation';
import { motion, type Motion } from './motion';

export type ThemeMode = 'light' | 'dark' | 'auto';

export interface Theme {
  mode: ThemeMode;
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
export const createTheme = (mode: ThemeMode, systemColorScheme: 'light' | 'dark'): Theme => {
  const isDark = mode === 'auto' ? systemColorScheme === 'dark' : mode === 'dark';

  return {
    mode,
    colors: buildScheme(isDark),
    isDark,
    spacing,
    radius,
    typography,
    elevation,
    motion,
  };
};

// 导出 token
export { lightColors, darkColors, spacing, radius, typography, elevation, motion, alpha, blend };
export type { ColorScheme, Spacing, Radius, Typography, Elevation, Motion };
