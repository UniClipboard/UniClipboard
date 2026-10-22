import { Toolbar as BaseToolbar } from '@base-ui/react/toolbar'
import { Tooltip } from '@base-ui/react/tooltip'
import { useState } from 'react'
import type { CSSProperties } from 'react'
import type {
  SurfaceProps,
  ToolbarButtonProps,
  ToolbarGroupProps,
  ToolbarProps,
  UiRootProps,
} from '@/ui-kit/contract'
import { useAppliedTheme } from '@/ui-kit/theme/applied-theme'
import { WithHelpTag } from './controls'
import { glyph } from './glyph'
import { PushButton } from './parts'
import { PortalContainer } from './portal'
import { linuxTheme } from './theme'
import './tokens.css'
import './adwaita.css'

/**
 * Theme scope of the Linux skin. Menus, tooltips and dialogs portal into this element instead of
 * <body>, so they keep the light/dark tokens of `data-adw-theme`. The window frame belongs to the
 * system (GTK client-side or server-side decorations), so the page draws no window chrome.
 */
export function UiRoot({ colorScheme, children }: UiRootProps) {
  const appliedTheme = useAppliedTheme()
  const [element, setElement] = useState<HTMLDivElement | null>(null)
  return (
    <div
      ref={setElement}
      className="adw-theme adw-root"
      style={linuxTheme(colorScheme, appliedTheme) as CSSProperties}
      data-adw-theme={colorScheme}
      data-ui-skin="linux"
    >
      <PortalContainer value={element}>
        <Tooltip.Provider delay={700}>{children}</Tooltip.Provider>
      </PortalContainer>
    </div>
  )
}

/**
 * A window region. The sidebar paints `--sidebar` and the content `--view`; both are opaque, so
 * `data-window-material` is always `solid` (the Linux host applies no window material).
 */
export function Surface({ region, children }: SurfaceProps) {
  return (
    <div className="adw-surface" data-surface={region} data-window-material="solid">
      {children}
    </div>
  )
}

/** Header bar items: one tab stop, arrow keys move between items. */
export function Toolbar({ label, children }: ToolbarProps) {
  return (
    <BaseToolbar.Root className="adw-toolbar-bar" aria-label={label}>
      {children}
    </BaseToolbar.Root>
  )
}

/** Items that belong together sit close together in one group. */
export function ToolbarGroup({ children }: ToolbarGroupProps) {
  return <BaseToolbar.Group className="adw-toolbar-group">{children}</BaseToolbar.Group>
}

export function ToolbarButton({
  variant,
  icon,
  children,
  label,
  tooltip,
  disabled,
  onClick,
}: ToolbarButtonProps) {
  const iconOnly = children === undefined || children === null
  return (
    <WithHelpTag text={tooltip ?? (iconOnly ? label : undefined)}>
      <BaseToolbar.Button
        render={
          <PushButton
            tint={variant === 'primary' ? 'primary' : iconOnly ? 'borderless' : 'none'}
            icon={icon && glyph(icon)}
            aria-label={iconOnly ? label : undefined}
            data-icon-only={iconOnly ? '' : undefined}
          />
        }
        disabled={disabled}
        onClick={onClick}
      >
        {children}
      </BaseToolbar.Button>
    </WithHelpTag>
  )
}

/** Header bar groups are told apart by spacing, not by a drawn line. */
export function ToolbarDivider() {
  return <BaseToolbar.Separator className="adw-toolbar-divider" />
}
