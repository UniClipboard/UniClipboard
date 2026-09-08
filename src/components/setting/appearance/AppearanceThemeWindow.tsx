import type { ThemeTokens } from '@/lib/theme-engine'

/** A static miniature of the desktop window, styled with the actual palette. */
export default function AppearanceThemeWindow({
  tokens,
  clipped = false,
}: {
  tokens: ThemeTokens
  clipped?: boolean
}) {
  return (
    <span
      className="appearance-theme-window"
      style={{
        backgroundColor: tokens.background,
        color: tokens.foreground,
        clipPath: clipped ? 'inset(0 0 0 50%)' : undefined,
      }}
    >
      <span
        className="appearance-theme-window-bar"
        style={{ backgroundColor: tokens.sidebar, borderColor: tokens.border }}
      >
        <span className="bg-[#ff6057]" />
        <span className="bg-[#febc2e]" />
        <span className="bg-[#28c840]" />
      </span>
      <span className="appearance-theme-window-body">
        <span
          className="appearance-theme-window-sidebar"
          style={{ backgroundColor: tokens.sidebar, borderColor: tokens.border }}
        >
          <span className="w-full" style={{ backgroundColor: tokens.primary }} />
          <span className="w-3/5" />
          <span className="w-1/3" />
        </span>
        <span className="appearance-theme-window-content">
          <span className="w-full" style={{ backgroundColor: tokens.primary }} />
          <span className="w-4/5" />
          <span className="w-3/5" />
        </span>
      </span>
    </span>
  )
}
