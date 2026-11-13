/**
 * Android 色板 —— Material 3 Expressive(单一 M3 baseline,light + dark)
 *
 * 实现 ColorScheme 中性契约,值全部为 M3 十六进制。
 * 命名对照(中性名 ← 原 M3 token):
 *   textPrimary  ← onSurface / onBackground
 *   textSecondary← onSurfaceVariant
 *   accent*      ← primary*
 *   surface*     ← surfaceContainer*
 *   border       ← outline
 *   separator    ← outlineVariant
 *
 * Metro 在 Android 平台自动解析到本文件;iOS 走 colors.ios.ts;其它环境(jest/tsc/web)走 colors.ts。
 *
 * Material You:Android 12+ 设备上 source tokens 取自系统壁纸色板(@expo/ui getMaterialColors),
 * 与 Compose <Host> 不传 seedColor 时的色板同源;设备不支持动态取色(或 jest 等无原生模块环境)
 * 时回落到下方品牌 baseline。Host 统一传 MATERIAL_SEED_COLOR,保证 RN 与 Compose 同一色源。
 */

import type { ColorScheme } from './colors.types';

import { alpha } from './colorUtils';

export { alpha, blend } from './colorUtils';
export type { ColorScheme, Color } from './colors.types';

// ------------------------------------------------------------------
// Source tokens(随深浅变化)
// ------------------------------------------------------------------

type SourceTokens = {
  accent: string;
  inverseAccent: string;
  onAccent: string;
  accentContainer: string;
  onAccentContainer: string;
  background: string;
  surface: string;
  surfaceLowest: string;
  surfaceLow: string;
  surfaceMid: string;
  surfaceHigh: string;
  surfaceHighest: string;
  textPrimary: string;
  textSecondary: string;
  border: string;
  separator: string;
  inverseSurface: string;
  inverseOnSurface: string;
};

const SOURCE_LIGHT: SourceTokens = {
  accent: '#6750A4',
  inverseAccent: '#D0BCFF',
  onAccent: '#FFFFFF',
  accentContainer: '#EADDFF',
  onAccentContainer: '#21005D',
  background: '#FEF7FF',
  surface: '#FEF7FF',
  surfaceLowest: '#FFFFFF',
  surfaceLow: '#F7F2FA',
  surfaceMid: '#F3EDF7',
  surfaceHigh: '#ECE6F0',
  surfaceHighest: '#E6E0E9',
  textPrimary: '#1D1B20',
  textSecondary: '#49454F',
  border: '#79747E',
  separator: '#CAC4D0',
  inverseSurface: '#322F35',
  inverseOnSurface: '#F5EFF7',
};

const SOURCE_DARK: SourceTokens = {
  accent: '#D0BCFF',
  inverseAccent: '#6750A4',
  onAccent: '#381E72',
  accentContainer: '#4F378B',
  onAccentContainer: '#EADDFF',
  background: '#141218',
  surface: '#141218',
  surfaceLowest: '#0F0D13',
  surfaceLow: '#1D1B20',
  surfaceMid: '#211F26',
  surfaceHigh: '#2B2930',
  surfaceHighest: '#36343B',
  textPrimary: '#E6E0E9',
  textSecondary: '#CAC4D0',
  border: '#938F99',
  separator: '#49454F',
  inverseSurface: '#E6E0E9',
  inverseOnSurface: '#322F35',
};

// ------------------------------------------------------------------
// Fixed tokens(不随 source 变化的语义/状态色)
// ------------------------------------------------------------------

const FIXED_LIGHT = {
  textDisabled: '#CAC4D0',

  error: '#B3261E',
  onError: '#FFFFFF',
  errorContainer: '#F9DEDC',
  onErrorContainer: '#410E0B',
  errorContainerBorder: '#F2B8B5',
  // M3 没有 warning/success/info 角色,按 M3 custom color 的 tone 40 / 90 取值
  warning: '#8B5000',
  onWarning: '#FFFFFF',
  warningContainer: '#FFDCBE',
  onWarningContainer: '#2C1600',
  warningContainerBorder: '#FFB870',
  success: '#146C2E',
  onSuccess: '#FFFFFF',
  successContainer: '#C4EFC6',
  onSuccessContainer: '#002107',
  successContainerBorder: '#A2D8A6',
  info: '#0B57D0',
  onInfo: '#FFFFFF',
  infoContainer: '#D3E3FD',
  onInfoContainer: '#041E49',
  infoContainerBorder: '#A8C7FA',

  overlay: 'rgba(0, 0, 0, 0.3)',
  backdrop: 'rgba(0, 0, 0, 0.32)',

  white: '#FFFFFF',
  transparent: 'transparent',
};

const FIXED_DARK = {
  textDisabled: '#49454F',

  error: '#F2B8B5',
  onError: '#601410',
  errorContainer: '#8C1D18',
  onErrorContainer: '#F9DEDC',
  errorContainerBorder: 'rgba(242, 184, 181, 0.35)',
  // tone 80 / 30
  warning: '#FFB870',
  onWarning: '#4A2800',
  warningContainer: '#693C00',
  onWarningContainer: '#FFDCBE',
  warningContainerBorder: 'rgba(255, 184, 112, 0.35)',
  success: '#88D98E',
  onSuccess: '#00390F',
  successContainer: '#00531D',
  onSuccessContainer: '#C4EFC6',
  successContainerBorder: 'rgba(136, 217, 142, 0.35)',
  info: '#A8C7FA',
  onInfo: '#062E6F',
  infoContainer: '#0842A0',
  onInfoContainer: '#D3E3FD',
  infoContainerBorder: 'rgba(168, 199, 250, 0.35)',

  overlay: 'rgba(255, 255, 255, 0.1)',
  backdrop: 'rgba(0, 0, 0, 0.32)',

  white: '#FFFFFF',
  transparent: 'transparent',
};

// ------------------------------------------------------------------
// Material You(动态取色)
// ------------------------------------------------------------------

/** `#RRGGBBAA` → 不透明时 `#RRGGBB`(下游 alpha()/渐变拼接依赖 6 位 hex),否则 rgba() */
function toColorString(rgba: string): string {
  if (rgba.length !== 9) return rgba;
  const a = rgba.slice(7, 9).toUpperCase();
  if (a === 'FF') return rgba.slice(0, 7).toUpperCase();
  const r = parseInt(rgba.slice(1, 3), 16);
  const g = parseInt(rgba.slice(3, 5), 16);
  const b = parseInt(rgba.slice(5, 7), 16);
  return `rgba(${r}, ${g}, ${b}, ${(parseInt(a, 16) / 255).toFixed(3)})`;
}

type ExpoUiColors = typeof import('@expo/ui/jetpack-compose');

/** 读取系统壁纸色板;设备不支持动态取色或原生模块不可用(jest)时返回 null */
function readDynamicSource(scheme: 'light' | 'dark'): SourceTokens | null {
  try {
    const ui = require('@expo/ui/jetpack-compose') as ExpoUiColors;
    if (!ui.isDynamicColorAvailable) return null;
    const m = ui.getMaterialColors({ scheme });
    return {
      accent: toColorString(m.primary),
      inverseAccent: toColorString(m.inversePrimary),
      onAccent: toColorString(m.onPrimary),
      accentContainer: toColorString(m.primaryContainer),
      onAccentContainer: toColorString(m.onPrimaryContainer),
      background: toColorString(m.background),
      surface: toColorString(m.surface),
      surfaceLowest: toColorString(m.surfaceContainerLowest),
      surfaceLow: toColorString(m.surfaceContainerLow),
      surfaceMid: toColorString(m.surfaceContainer),
      surfaceHigh: toColorString(m.surfaceContainerHigh),
      surfaceHighest: toColorString(m.surfaceContainerHighest),
      textPrimary: toColorString(m.onSurface),
      textSecondary: toColorString(m.onSurfaceVariant),
      border: toColorString(m.outline),
      separator: toColorString(m.outlineVariant),
      inverseSurface: toColorString(m.inverseSurface),
      inverseOnSurface: toColorString(m.inverseOnSurface),
    };
  } catch {
    return null;
  }
}

const DYNAMIC_LIGHT = readDynamicSource('light');
const DYNAMIC_DARK = readDynamicSource('dark');

/** 当前 RN 色板是否来自系统壁纸(Material You) */
export const isUsingDynamicColor = DYNAMIC_LIGHT != null && DYNAMIC_DARK != null;

/**
 * Compose <Host seedColor> 的统一取值:动态取色时为 undefined(Host 跟随壁纸,与 RN 同源),
 * 否则为品牌色(Host 由同一品牌 seed 生成的色板即 M3 baseline,与 SOURCE_LIGHT/DARK 一致)。
 */
export const MATERIAL_SEED_COLOR: string | undefined = isUsingDynamicColor
  ? undefined
  : SOURCE_LIGHT.accent;

// ------------------------------------------------------------------
// Builders / exports
// ------------------------------------------------------------------

function composeScheme(s: SourceTokens, f: typeof FIXED_LIGHT): ColorScheme {
  // M3 state layer:以 onSurface 叠加不同不透明度(pressed 10% / dragged 16%)
  const stateLayer = (opacity: number) => alpha(s.textPrimary, opacity) as string;
  return {
    textPrimary: s.textPrimary,
    textSecondary: s.textSecondary,
    textTertiary: s.border,
    textDisabled: f.textDisabled,

    accent: s.accent,
    inverseAccent: s.inverseAccent,
    onAccent: s.onAccent,
    accentContainer: s.accentContainer,
    onAccentContainer: s.onAccentContainer,

    background: s.background,
    surface: s.surface,
    surfaceLowest: s.surfaceLowest,
    surfaceLow: s.surfaceLow,
    surfaceMid: s.surfaceMid,
    surfaceHigh: s.surfaceHigh,
    surfaceHighest: s.surfaceHighest,

    border: s.border,
    separator: s.separator,

    error: f.error,
    onError: f.onError,
    errorContainer: f.errorContainer,
    onErrorContainer: f.onErrorContainer,
    errorContainerBorder: f.errorContainerBorder,
    success: f.success,
    onSuccess: f.onSuccess,
    successContainer: f.successContainer,
    onSuccessContainer: f.onSuccessContainer,
    successContainerBorder: f.successContainerBorder,
    warning: f.warning,
    onWarning: f.onWarning,
    warningContainer: f.warningContainer,
    onWarningContainer: f.onWarningContainer,
    warningContainerBorder: f.warningContainerBorder,
    info: f.info,
    onInfo: f.onInfo,
    infoContainer: f.infoContainer,
    onInfoContainer: f.onInfoContainer,
    infoContainerBorder: f.infoContainerBorder,

    inverseSurface: s.inverseSurface,
    inverseOnSurface: s.inverseOnSurface,

    overlay: f.overlay,
    backdrop: f.backdrop,
    messageSuccess: f.success,
    messageError: f.error,
    white: f.white,
    transparent: f.transparent,

    fillPrimary: stateLayer(0.16),
    fillSecondary: stateLayer(0.12),
    fillTertiary: stateLayer(0.1),
    fillQuaternary: stateLayer(0.08),
  };
}

export function buildScheme(isDark: boolean): ColorScheme {
  return isDark
    ? composeScheme(DYNAMIC_DARK ?? SOURCE_DARK, FIXED_DARK)
    : composeScheme(DYNAMIC_LIGHT ?? SOURCE_LIGHT, FIXED_LIGHT);
}

export const lightColors = buildScheme(false);
export const darkColors = buildScheme(true);
