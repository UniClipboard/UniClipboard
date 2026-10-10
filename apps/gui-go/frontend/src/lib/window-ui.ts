import { applyPlatformEffectPreferences } from '@/lib/platform'
import { initializeScrollbarVisibility } from '@/lib/scrollbar-visibility'
import { initializeUiScale } from '@/lib/ui-scale'
import { initializeUiSound } from '@/lib/ui-sound'
import { initializeVisualEffects } from '@/lib/visual-effects-store'

export { applyPlatformEffectPreferences } from '@/lib/platform'

export const initializeWindowUi = (): (() => void) => {
  applyPlatformEffectPreferences()
  const disposeVisualEffects = initializeVisualEffects()
  const disposeScrollbarVisibility = initializeScrollbarVisibility()
  const disposeUiScale = initializeUiScale()
  const disposeUiSound = initializeUiSound()

  return () => {
    disposeVisualEffects()
    disposeUiSound()
    disposeUiScale()
    disposeScrollbarVisibility()
  }
}
