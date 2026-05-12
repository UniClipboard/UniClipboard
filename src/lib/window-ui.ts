import { applyPlatformEffectPreferences, detectPlatformInfo } from '@/lib/platform'
import { initializeUiScale } from '@/lib/ui-scale'

export { applyPlatformEffectPreferences } from '@/lib/platform'

export const applyPlatformTypographyScale = () => {
  if (typeof document === 'undefined') {
    return
  }

  const { isWindows } = detectPlatformInfo()

  if (!isWindows) {
    return
  }

  const root = document.documentElement

  root.style.setProperty('--font-size-caption', '0.6875rem') /* 11px */
  root.style.setProperty('--font-size-small', '0.75rem') /* 12px */
  root.style.setProperty('--font-size-body', '0.8125rem') /* 13px */
  root.style.setProperty('--font-size-body-lg', '0.875rem') /* 14px */
  root.style.setProperty('--font-size-section', '0.9375rem') /* 15px */
  root.style.setProperty('--font-size-title', '1.125rem') /* 18px */
}

export const initializeWindowUi = (): (() => void) => {
  applyPlatformTypographyScale()
  applyPlatformEffectPreferences()
  return initializeUiScale()
}
