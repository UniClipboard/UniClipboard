import { Toolbar as BaseToolbar } from '@base-ui/react/toolbar'
import { Tooltip } from '@base-ui/react/tooltip'
import { createContext, use, useEffect, useState, useSyncExternalStore } from 'react'
import { subscribeWindowMaterial, type WindowMaterial } from '@/host/window-material'
import type {
  SurfaceProps,
  ToolbarButtonProps,
  ToolbarGroupProps,
  ToolbarProps,
  UiRootProps,
} from '@/ui-kit/contract'
import { WithHelpTag } from './controls'
import { glyph } from './glyph'
import { Icon } from './icons'
import { PushButton } from './parts'
import { PortalContainer } from './portal'
import './tokens.css'
import './macos.css'

const AppliedMaterial = createContext<WindowMaterial>('none')

function subscribeKeyWindow(onChange: () => void) {
  window.addEventListener('focus', onChange)
  window.addEventListener('blur', onChange)
  return () => {
    window.removeEventListener('focus', onChange)
    window.removeEventListener('blur', onChange)
  }
}

/**
 * Web approximation of AppKit's key window: whether the document has focus. Selections turn gray
 * while it is false, as in an inactive NSWindow.
 */
const useKeyWindow = () =>
  useSyncExternalStore(
    subscribeKeyWindow,
    () => document.hasFocus(),
    () => true
  )

const darkQuery = '(prefers-color-scheme: dark)'

function subscribeSystemScheme(onChange: () => void) {
  const media = window.matchMedia(darkQuery)
  media.addEventListener('change', onChange)
  return () => media.removeEventListener('change', onChange)
}

/**
 * The appearance AppKit draws the window material in. The host leaves the window on the system
 * appearance (Wails sets NSAppearance only at creation), and WKWebView reports that appearance as
 * `prefers-color-scheme`.
 */
const useMaterialScheme = () =>
  useSyncExternalStore(
    subscribeSystemScheme,
    () => (window.matchMedia(darkQuery).matches ? 'dark' : 'light'),
    () => 'light'
  )

/**
 * Theme scope of the macOS skin. Menus, tooltips and alerts portal into this element instead of
 * <body>, so they keep the light/dark tokens of `data-mac-theme`. When the host reports a native
 * window material in the app's appearance, the page background is cleared so the material shows
 * behind sidebar surfaces.
 */
export function UiRoot({ colorScheme, children }: UiRootProps) {
  const [element, setElement] = useState<HTMLDivElement | null>(null)
  const [hostMaterial, setMaterial] = useState<WindowMaterial>('none')
  const keyWindow = useKeyWindow()
  // An app theme that differs from the system appearance would put light content on dark glass (or
  // the reverse), so the page then covers the material and the sidebar uses its CSS fallback.
  const material = useMaterialScheme() === colorScheme ? hostMaterial : 'none'
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
      data-window-inactive={keyWindow ? undefined : ''}
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
export function Surface({ region, children }: SurfaceProps) {
  const material = use(AppliedMaterial)
  const marker =
    region === 'content' ? 'solid' : material === 'none' ? 'css-fallback' : `native-${material}`
  return (
    <div className="mac-surface" data-surface={region} data-window-material={marker}>
      {children}
    </div>
  )
}

/** Icon-only toolbar items, which Tahoe groups into one glass capsule. */
/** Tahoe toolbar: one tab stop, arrow keys move between items, controls float as glass capsules. */
export function Toolbar({ label, children }: ToolbarProps) {
  return (
    <BaseToolbar.Root className="mac-toolbar-bar" aria-label={label}>
      {children}
    </BaseToolbar.Root>
  )
}

/** Items that belong together share one glass capsule (design MA2: Send, Pin, More). */
export function ToolbarGroup({ children }: ToolbarGroupProps) {
  return (
    <BaseToolbar.Group className="mac-glass-group mac-glass mac-toolbar-capsule">
      {children}
    </BaseToolbar.Group>
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
