import i18n from '@/i18n'

// The UI-kit skin the build was made for, read from the rendered root. The Linux skin mirrors the
// macOS class suffixes under its own prefix, so one selector set serves both.
const CLASS_PREFIX: Record<string, string> = { macos: 'mac', linux: 'adw' }

export const skinName = () =>
  document.querySelector<HTMLElement>('[data-ui-skin]')?.dataset.uiSkin ?? null

/** `.mac-<suffix>` on macOS, `.adw-<suffix>` on Linux (queried at call time, after the root rendered). */
export const skinClass = (suffix: string) =>
  `.${CLASS_PREFIX[skinName() ?? 'macos'] ?? 'mac'}-${suffix}`

/** The platform modifier of the page's shortcuts: Command on macOS, Control elsewhere. */
export const modifier = (): { metaKey: true } | { ctrlKey: true } =>
  /Mac|iPhone|iPad|iPod/.test(navigator.userAgent) ? { metaKey: true } : { ctrlKey: true }

/** The skin this OS must be given, from the engine's own user agent (independent of the rendered DOM). */
export const expectedSkin = () =>
  /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)
    ? 'macos'
    : /Linux/.test(navigator.userAgent)
      ? 'linux'
      : 'windows'

/**
 * The Settings page is open: the existing page's header, or (the kit page has no such header) the
 * /settings route with its category list. Both pages list the same categories as buttons.
 */
export const settingsPageOpen = () =>
  !!document.querySelector('[data-testid="settings-page-header"]') ||
  (location.pathname.startsWith('/settings') &&
    Array.from(document.querySelectorAll<HTMLElement>('button')).some(
      b => b.textContent?.trim() === i18n.t('settings.categories.appearance')
    ))
