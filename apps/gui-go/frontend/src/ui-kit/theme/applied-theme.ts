import { useSyncExternalStore } from 'react'
import type { ThemeTokens } from '@/lib/theme-engine'
import { THEME_TOKEN_CSS_VARIABLES } from '@/lib/theme-token-variables'

export interface AppliedTheme {
  name: string
  customized: boolean
  colors: ThemeTokens
}

export function isDefaultTheme(theme: AppliedTheme | null): boolean {
  return !theme || (theme.name === 'default' && !theme.customized)
}

function subscribe(onChange: () => void) {
  const observer = new MutationObserver(onChange)
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['style', 'data-theme', 'data-theme-customized'],
  })
  return () => observer.disconnect()
}

function snapshot(): string {
  const root = document.documentElement
  if (!root.dataset.theme) return ''
  return JSON.stringify({
    name: root.dataset.theme,
    customized: root.dataset.themeCustomized === 'true',
    colors: Object.fromEntries(
      Object.entries(THEME_TOKEN_CSS_VARIABLES).map(([key, variable]) => [
        key,
        root.style.getPropertyValue(variable).trim(),
      ])
    ),
  })
}

/** All skins observe the palette already applied by the window's single theme owner. */
export function useAppliedTheme(): AppliedTheme | null {
  const value = useSyncExternalStore(subscribe, snapshot, () => '')
  return value ? (JSON.parse(value) as AppliedTheme) : null
}
