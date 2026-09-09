import { subscribeDesktopTheme } from '@/lib/desktop-theme'
import type { DesktopTheme } from '@/lib/desktop-theme'
import { applyThemeOverrides, applyThemePreset, DEFAULT_THEME_COLOR } from '@/lib/theme-engine'
import { startThemeTransition } from '@/lib/theme-transition'
import type { Settings } from '@/types/setting'

type General = Settings['general'] | null | undefined

/** One theme owner per WebView; platform integrations only supply a palette. */
export function createWindowThemeController(animate = false) {
  const root = document.documentElement
  const media = window.matchMedia('(prefers-color-scheme: dark)')
  let general: General
  let ready = false
  let desktop: DesktopTheme | null = null
  let previous: string | undefined
  let generation = 0
  let disposed = false

  const refresh = (animateChange = false) => {
    if (!ready || disposed) return
    const preference = general?.theme
    const manual = preference === 'light' || preference === 'dark'
    const external = manual ? null : desktop
    const mode = manual ? preference : (external?.dark ?? media.matches) ? 'dark' : 'light'
    const split = mode === 'dark' ? general?.themeColorDark : general?.themeColorLight
    const preset = split || general?.themeColor || DEFAULT_THEME_COLOR
    const overrides =
      (mode === 'dark' ? general?.themeOverridesDark : general?.themeOverridesLight) ?? {}
    const signature = JSON.stringify(
      external ? [mode, external.variables] : [mode, preset, overrides]
    )
    if (signature === previous) return
    const currentGeneration = ++generation
    const apply = () => {
      if (disposed || currentGeneration !== generation) return
      root.classList.remove('light', 'dark')
      root.classList.add(mode)
      if (external) {
        for (const [key, value] of Object.entries(external.variables))
          root.style.setProperty(key, value)
        root.setAttribute('data-theme', 'desktop')
      } else {
        applyThemePreset(preset, mode, root)
        applyThemeOverrides(overrides, root)
      }
    }
    if (animate && animateChange && previous !== undefined) startThemeTransition(apply)
    else apply()
    previous = signature
  }
  const unsubscribe = subscribeDesktopTheme(theme => {
    desktop = theme
    refresh()
  })
  const handleSystemChange = () => refresh()
  media.addEventListener('change', handleSystemChange)
  return {
    setGeneral(next: General) {
      general = next
      ready = true
      refresh(true)
    },
    dispose() {
      disposed = true
      unsubscribe()
      media.removeEventListener('change', handleSystemChange)
    },
  }
}
