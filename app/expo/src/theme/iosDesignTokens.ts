import { Platform, PlatformColor, DynamicColorIOS, type OpaqueColorValue } from 'react-native';

/**
 * iOS Liquid Glass design tokens — mirrors DESIGN.md from the native project.
 * Only used on iOS; Android continues with M3 tokens.
 */

// -- Dimensions (from DESIGN.md) --

export const iosDimensions = {
  floatingButtonSize: 52,
  sheetFilledCapsuleHeight: 48,
  actionBarButtonHeight: 44,
  minTapTarget: 44,
  cardCornerRadius: 14,
  surfaceCornerRadius: 12,
  glassCardRadius: 18,
  gridSpacing: 12,
  screenHPadding: 16,
  sheetHPadding: 20,
  cardInnerPadding: 12,
  panelInnerPadding: 14,
  gridAdaptiveMin: 160,
  gridAdaptiveMax: 210,
  latestDotSize: 6,
  statusDotSize: 6,
} as const;

// -- Kind tints (semantic per-clipboard-kind colors) --

export const iosKindTints = {
  text: '#007AFF', // system blue
  url: '#32ADE6', // system cyan
  image: '#34C759', // system green
  file: '#FF9500', // system orange
  group: '#AF52DE', // system purple
} as const;

// -- System colors via PlatformColor (auto light/dark) --

export function iosSystemColor(name: string) {
  if (Platform.OS !== 'ios') return undefined;
  return PlatformColor(name);
}

export const iosColors =
  Platform.OS === 'ios'
    ? {
        systemGroupedBackground: PlatformColor('systemGroupedBackground'),
        secondarySystemGroupedBackground: PlatformColor('secondarySystemGroupedBackground'),
        tertiarySystemGroupedBackground: PlatformColor('tertiarySystemGroupedBackground'),
        separator: PlatformColor('separator'),
        label: PlatformColor('label'),
        secondaryLabel: PlatformColor('secondaryLabel'),
        tertiaryLabel: PlatformColor('tertiaryLabel'),
        quaternaryLabel: PlatformColor('quaternaryLabel'),
        systemBackground: PlatformColor('systemBackground'),
        secondarySystemBackground: PlatformColor('secondarySystemBackground'),
        tertiarySystemFill: PlatformColor('tertiarySystemFill'),
      }
    : null;

// -- Accent (from DESIGN.md) --

export const iosAccent = {
  light: '#15171C',
  dark: '#F4F2EE',
} as const;

export const iosAccentColor: OpaqueColorValue | undefined =
  Platform.OS === 'ios'
    ? DynamicColorIOS({ light: iosAccent.light, dark: iosAccent.dark })
    : undefined;

/** accent 填充上的前景色(明/暗手动解析,给吃不了 PlatformColor 的场景用) */
export const iosOnAccent = {
  light: '#FFFFFF',
  dark: iosAccent.light,
} as const;

export const iosOnAccentColor: OpaqueColorValue | undefined =
  Platform.OS === 'ios'
    ? DynamicColorIOS({ light: iosOnAccent.light, dark: iosOnAccent.dark })
    : undefined;

/** Foreground for fixed saturated button fills in both appearances. */
export const iosOnSaturatedColor = '#FFFFFF';

// -- 系统色的明暗 hex 近似 --
// 优先用上面的 iosColors(PlatformColor);只有 SwiftUI modifier(@expo/ui)和
// CSS 渐变(experimental_backgroundImage)这类吃不了 PlatformColor 的出口才用这份。
// 值对照 UIKit 的默认明暗解析结果,新增条目时同样成对给出 light/dark。

export const iosSystemHex = {
  /** label */
  label: { light: '#000000', dark: '#FFFFFF' },
  /** systemGroupedBackground */
  groupedBackground: { light: '#F2F2F7', dark: '#000000' },
  /** secondarySystemGroupedBackground(分组页里的卡片/胶囊底色) */
  secondaryGroupedBackground: { light: '#FFFFFF', dark: '#1C1C1E' },
  /** secondaryLabel */
  secondaryLabel: { light: 'rgba(60, 60, 67, 0.6)', dark: 'rgba(235, 235, 245, 0.6)' },
} as const;

// -- Card shadow (from DESIGN.md) --

export const iosCardShadow = {
  shadowColor: '#000',
  shadowOffset: { width: 0, height: 2 },
  shadowOpacity: 0.06,
  shadowRadius: 5,
} as const;

// -- Helpers --

export function hexToRgba(hex: string, alpha: number): string {
  const clean = hex.replace('#', '');
  const r = parseInt(clean.substring(0, 2), 16);
  const g = parseInt(clean.substring(2, 4), 16);
  const b = parseInt(clean.substring(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
