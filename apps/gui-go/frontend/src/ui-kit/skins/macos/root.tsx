import { Toolbar as BaseToolbar } from '@base-ui/react/toolbar'
import { Tooltip } from '@base-ui/react/tooltip'
import { createContext, use, useEffect, useState } from 'react'
import { subscribeWindowMaterial, type WindowMaterial } from '@/host/window-material'
import type { SurfaceProps, ToolbarButtonProps, ToolbarProps, UiRootProps } from '@/ui-kit/contract'
import { WithHelpTag } from './controls'
import { glyph } from './glyph'
import { Icon } from './icons'
import { PushButton } from './parts'
import { PortalContainer } from './portal'
import './tokens.css'
import './macos.css'

const AppliedMaterial = createContext<WindowMaterial>('none')

/**
 * Theme scope of the macOS skin. Menus, tooltips and alerts portal into this element instead of
 * <body>, so they keep the light/dark tokens of `data-mac-theme`. When the host reports a native
 * window material, the page background is cleared so the material shows behind sidebar surfaces.
 */
export function UiRoot({ colorScheme, children }: UiRootProps) {
  const [element, setElement] = useState<HTMLDivElement | null>(null)
  const [material, setMaterial] = useState<WindowMaterial>('none')
  useEffect(() => {
    const unsubscribe = subscribeWindowMaterial(setMaterial)
    return unsubscribe
  }, [])
  useEffect(() => {
    if (material === 'none') return
    // The document itself must be transparent for the window material to show through.
    document.documentElement.dataset.windowMaterial = `native-${material}`
    return () => {
      delete document.documentElement.dataset.windowMaterial
    }
  }, [material])
  return (
    <div
      ref={setElement}
      className="mac-theme mac-root"
      data-mac-theme={colorScheme}
      data-ui-skin="macos"
      data-native-material={material === 'none' ? undefined : ''}
    >
      <PortalContainer value={element}>
        <AppliedMaterial value={material}>
          <Tooltip.Provider delay={700}>{children}</Tooltip.Provider>
        </AppliedMaterial>
      </PortalContainer>
    </div>
  )
}

/**
 * A window region. `sidebar` shows the host's window material (NSGlassEffectView on macOS 26+,
 * NSVisualEffectView before) when the host applied one, and the CSS approximation otherwise;
 * `content` is always solid. `data-window-material` says which one is on screen.
 */
export function Surface({ role, children }: SurfaceProps) {
  const material = use(AppliedMaterial)
  const marker =
    role === 'content' ? 'solid' : material === 'none' ? 'css-fallback' : `native-${material}`
  return (
    <div className="mac-surface" data-surface={role} data-window-material={marker}>
      {children}
    </div>
  )
}

/** Tahoe toolbar: one tab stop, arrow keys move between items, controls float as glass capsules. */
export function Toolbar({ label, children }: ToolbarProps) {
  return (
    <BaseToolbar.Root className="mac-toolbar-bar" aria-label={label}>
      {children}
    </BaseToolbar.Root>
  )
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
  if (iconOnly && icon) {
    return (
      <span className="mac-glass-group mac-glass">
        <WithHelpTag text={tooltip ?? label}>
          <BaseToolbar.Button
            className="mac-glass-item"
            aria-label={label}
            disabled={disabled}
            onClick={onClick}
          >
            <Icon name={glyph(icon)} size={16} strokeWidth={1.7} />
          </BaseToolbar.Button>
        </WithHelpTag>
      </span>
    )
  }
  return (
    <WithHelpTag text={tooltip}>
      <BaseToolbar.Button
        render={
          <PushButton
            size="large"
            tint={variant === 'primary' ? 'primary' : 'glass'}
            icon={icon && glyph(icon)}
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

/** Tahoe separates toolbar groups with space between capsules, not with a drawn line. */
export function ToolbarDivider() {
  return <BaseToolbar.Separator className="mac-toolbar-divider" />
}
