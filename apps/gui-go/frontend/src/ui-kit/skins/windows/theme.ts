import { createDarkTheme, createLightTheme } from '@fluentui/react-components'
import type { BrandVariants, Theme } from '@fluentui/react-components'
import type { ColorScheme, TagColor, TagPaletteColor } from '@/ui-kit/contract'

// Windows 11 default accent ramp. Shade 80 is the light-theme accent fill (AccentFillColorDefault
// #005FB8); 130 and 150 are the dark-theme accent fill and accent text (#60CDFF, #99EBFF).
const accent: BrandVariants = {
  10: '#001033',
  20: '#001A68',
  30: '#00267A',
  40: '#003E92',
  50: '#004A9F',
  60: '#0052A8',
  70: '#0059B0',
  80: '#005FB8',
  90: '#0078D4',
  100: '#0091F8',
  110: '#2AA6FA',
  120: '#4CBAFD',
  130: '#60CDFF',
  140: '#7DD8FF',
  150: '#99EBFF',
  160: '#C7F4FF',
}

const fontFamilyBase =
  "'Segoe UI Variable Text', 'Segoe UI', system-ui, -apple-system, 'Noto Sans SC Variable', sans-serif"
const fontFamilyMonospace = "'Cascadia Mono', Consolas, 'JetBrains Mono', monospace"

// Values from the Win11 colour and CSS token boards. Translucent WinUI fills are flattened onto the
// base layer (#F3F3F3 / #202020) because Fluent web also uses these tokens for popups, which must
// stay opaque.
const light: Theme = {
  ...createLightTheme(accent),
  fontFamilyBase,
  fontFamilyMonospace,
  colorNeutralForeground1: 'rgba(0, 0, 0, 0.894)',
  colorNeutralForeground2: 'rgba(0, 0, 0, 0.62)',
  colorNeutralForeground3: 'rgba(0, 0, 0, 0.446)',
  colorNeutralForegroundDisabled: 'rgba(0, 0, 0, 0.361)',
  colorNeutralBackground1: '#FBFBFB',
  colorNeutralBackground1Hover: '#F6F6F6',
  colorNeutralBackground1Pressed: '#F5F5F5',
  colorNeutralStroke1: '#E5E5E5',
  colorNeutralStroke1Hover: '#E5E5E5',
  colorNeutralStroke1Pressed: '#E5E5E5',
  colorNeutralStrokeAccessible: 'rgba(0, 0, 0, 0.446)',
  colorSubtleBackgroundHover: 'rgba(0, 0, 0, 0.0373)',
  colorSubtleBackgroundPressed: 'rgba(0, 0, 0, 0.0241)',
  colorBrandBackgroundHover: 'rgba(0, 95, 184, 0.9)',
  colorBrandBackgroundPressed: 'rgba(0, 95, 184, 0.8)',
  colorPaletteRedForeground1: '#C42B1C',
}

const dark: Theme = {
  ...createDarkTheme(accent),
  fontFamilyBase,
  fontFamilyMonospace,
  colorNeutralForeground1: '#FFFFFF',
  colorNeutralForeground2: 'rgba(255, 255, 255, 0.786)',
  colorNeutralForeground3: 'rgba(255, 255, 255, 0.545)',
  colorNeutralForegroundDisabled: 'rgba(255, 255, 255, 0.363)',
  colorNeutralBackground1: '#2D2D2D',
  colorNeutralBackground1Hover: '#323232',
  colorNeutralBackground1Pressed: '#272727',
  colorNeutralStroke1: '#303030',
  colorNeutralStroke1Hover: '#303030',
  colorNeutralStroke1Pressed: '#303030',
  colorNeutralStrokeAccessible: 'rgba(255, 255, 255, 0.545)',
  colorSubtleBackgroundHover: 'rgba(255, 255, 255, 0.0605)',
  colorSubtleBackgroundPressed: 'rgba(255, 255, 255, 0.0419)',
  // Win11 dark accent controls are light blue with dark text.
  colorBrandBackground: accent[130],
  colorBrandBackgroundHover: 'rgba(96, 205, 255, 0.9)',
  colorBrandBackgroundPressed: 'rgba(96, 205, 255, 0.8)',
  colorBrandBackgroundSelected: accent[130],
  colorNeutralForegroundOnBrand: '#000000',
  colorBrandForeground1: accent[130],
  colorBrandForeground2: accent[150],
  colorCompoundBrandBackground: accent[130],
  colorCompoundBrandBackgroundHover: 'rgba(96, 205, 255, 0.9)',
  colorCompoundBrandBackgroundPressed: 'rgba(96, 205, 255, 0.8)',
  colorCompoundBrandStroke: accent[130],
  colorCompoundBrandStrokeHover: 'rgba(96, 205, 255, 0.9)',
  colorCompoundBrandStrokePressed: 'rgba(96, 205, 255, 0.8)',
  colorPaletteRedForeground1: '#FF99A4',
}

/**
 * Skin-local values Fluent has no token for (layer colours, elevation border, tags). They travel in
 * the theme object because FluentProvider writes every theme key as a CSS variable on its root and
 * on every portal mount node; inline styles on the provider root would not reach popups.
 */
const skinTokens: Record<ColorScheme, Record<string, string>> = {
  light: {
    'w11-base': '#F3F3F3',
    'w11-stroke-elevation': 'rgba(0, 0, 0, 0.1622)',
    'w11-acrylic': 'rgba(252, 252, 252, 0.85)',
    'w11-card': 'rgba(255, 255, 255, 0.7)',
    'w11-selected': 'rgba(0, 0, 0, 0.0373)',
    'w11-card-stroke': 'rgba(0, 0, 0, 0.0578)',
    'w11-notice-info': 'rgba(246, 246, 246, 0.5)',
    'w11-notice-success': '#DFF6DD',
    'w11-notice-warning': '#FFF4CE',
    'w11-notice-error': '#FDE7E9',
    'w11-tag-orange-bg': '#FBE6D8',
    'w11-tag-orange-ink': '#8A3F14',
    'w11-tag-blue-bg': '#E1ECF8',
    'w11-tag-blue-ink': '#1F4A8A',
    'w11-tag-gray-bg': '#EBEBEB',
    'w11-tag-gray-ink': '#3A3A3A',
    'w11-tag-purple-bg': '#F1E4F6',
    'w11-tag-purple-ink': '#6B2F80',
    'w11-tag-green-bg': '#DFF2E4',
    'w11-tag-green-ink': '#1D6B3A',
    'w11-tag-neutral-bg': '#EBEBEB',
    'w11-tag-neutral-ink': '#3A3A3A',
    // The card layer flattened onto the base layer, for sticky content that must hide what scrolls under it.
    'w11-card-solid': '#FBFBFB',
    // The detail column of the content card: the card over a subtle fill (board WA2).
    'w11-detail': 'rgba(0, 0, 0, 0.0122)',
    'w11-divider': 'rgba(0, 0, 0, 0.0803)',
    // WinUI SystemFillColor{Success,Caution,Critical,Neutral}.
    'w11-status-positive': '#0F7B0F',
    'w11-status-warning': '#9D5D00',
    'w11-status-critical': '#C42B1C',
    'w11-status-neutral': 'rgba(0, 0, 0, 0.446)',
  },
  dark: {
    'w11-base': '#202020',
    'w11-stroke-elevation': 'rgba(255, 255, 255, 0.093)',
    'w11-acrylic': 'rgba(44, 44, 44, 0.9)',
    'w11-card': 'rgba(255, 255, 255, 0.0512)',
    'w11-selected': 'rgba(255, 255, 255, 0.0605)',
    'w11-card-stroke': 'rgba(0, 0, 0, 0.1)',
    'w11-notice-info': 'rgba(255, 255, 255, 0.0326)',
    'w11-notice-success': '#393D1B',
    'w11-notice-warning': '#433519',
    'w11-notice-error': '#442726',
    'w11-tag-orange-bg': '#4A2E1E',
    'w11-tag-orange-ink': '#FFB98C',
    'w11-tag-blue-bg': '#1F3247',
    'w11-tag-blue-ink': '#9CC8FF',
    'w11-tag-gray-bg': '#3A3A3A',
    'w11-tag-gray-ink': '#D6D6D6',
    'w11-tag-purple-bg': '#3B2745',
    'w11-tag-purple-ink': '#E3B5F2',
    'w11-tag-green-bg': '#1E3A28',
    'w11-tag-green-ink': '#8FDCA6',
    'w11-tag-neutral-bg': '#3A3A3A',
    'w11-tag-neutral-ink': '#D6D6D6',
    'w11-card-solid': '#2B2B2B',
    'w11-detail': 'rgba(0, 0, 0, 0.08)',
    'w11-divider': 'rgba(255, 255, 255, 0.0837)',
    'w11-status-positive': '#6CCB5F',
    'w11-status-warning': '#FCE100',
    'w11-status-critical': '#FF99A4',
    'w11-status-neutral': 'rgba(255, 255, 255, 0.545)',
  },
}

export const themes: Record<ColorScheme, Theme> = {
  light: { ...light, ...skinTokens.light },
  dark: { ...dark, ...skinTokens.dark },
}

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
