import { isTauri } from '@/host/core'
import { getCurrentWindow } from '@/host/window'
import { subscribeDesktopTheme } from '@/lib/desktop-theme'
import type { DesktopTheme } from '@/lib/desktop-theme'
import { applyThemeOverrides, applyThemePreset, DEFAULT_THEME_COLOR } from '@/lib/theme-engine'
import { writeCachedThemeMode } from '@/lib/theme-mode-cache'
import { startThemeTransition } from '@/lib/theme-transition'
import type { Settings } from '@/types/setting'

type General = Settings['general'] | null | undefined

/** Resolve any CSS color (including oklch) to an RGBA tuple via a canvas. */
function resolveCssColor(css: string): [number, number, number, number] | null {
  const value = css.trim()
  if (!value) return null
  const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true })
  if (!ctx) return null
  ctx.clearRect(0, 0, 1, 1)
  ctx.fillStyle = '#000'
  ctx.fillStyle = value
  ctx.fillRect(0, 0, 1, 1)
  const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data
  return [r, g, b, a]
}

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
  let resolveReady!: () => void
  const initialized = new Promise<void>(resolve => {
    resolveReady = resolve
  })

  const refresh = (animateChange = false) => {
    if (!ready || disposed) return
    const preference = general?.theme
    const manual = preference === 'light' || preference === 'dark'
    const external = desktop
    const mode = external
      ? external.dark
        ? 'dark'
        : 'light'
      : manual
        ? preference
        : media.matches
          ? 'dark'
          : 'light'
    const split = mode === 'dark' ? general?.themeColorDark : general?.themeColorLight
    const preset = split || general?.themeColor || DEFAULT_THEME_COLOR
    const overrides =
      (mode === 'dark' ? general?.themeOverridesDark : general?.themeOverridesLight) ?? {}
    const signature = JSON.stringify(
      external ? [mode, external.variables ?? {}] : [mode, preset, overrides]
    )
    if (signature === previous) return
    const currentGeneration = ++generation
    const apply = () => {
      if (disposed || currentGeneration !== generation) return
      root.classList.remove('light', 'dark')
      root.classList.add(mode)
      writeCachedThemeMode(mode)
      if (external) {
        for (const [key, value] of Object.entries(external.variables ?? {})) {
          if (value !== undefined) root.style.setProperty(key, value)
        }
        root.setAttribute('data-theme', 'desktop')
      } else {
        applyThemePreset(preset, mode, root)
        applyThemeOverrides(overrides, root)
      }
      // The window is opaque; paint it with the page background so any area the webview does
      // not cover (OS-driven resize, hidden-window restore) matches the page.
      if (animate && isTauri()) {
        const win = getCurrentWindow()
        win.setTheme(external ? mode : manual ? preference : null).catch(() => {})
        const color = resolveCssColor(getComputedStyle(root).getPropertyValue('--background'))
        if (color) win.setBackgroundColor(color).catch(() => {})
      }
    }
    if (animate && animateChange && previous !== undefined) startThemeTransition(apply)
    else apply()
    previous = signature
  }
  const unsubscribe = subscribeDesktopTheme((theme, windowCornerRadius) => {
    root.style.setProperty('--desktop-window-radius', `${windowCornerRadius ?? 0}px`)
    desktop = theme
    refresh()
    resolveReady()
  })
  const handleSystemChange = () => refresh()
  media.addEventListener('change', handleSystemChange)
  return {
    initialized,
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

/** Apply the first desktop palette before mounting either WebView's visible content. */
export async function initializeWindowTheme(): Promise<void> {
  const controller = createWindowThemeController()
  await controller.initialized
  controller.setGeneral(null)
  // React takes ownership next; subscriptions replay the in-memory snapshot synchronously.
  controller.dispose()
}
