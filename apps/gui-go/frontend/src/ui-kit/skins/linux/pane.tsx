import type {
  ActionBarProps,
  ColorDotProps,
  ContentUnavailableProps,
  IconProps,
  InsetListProps,
  KindBadgeProps,
  PaneProps,
  SplitViewProps,
  TextProps,
} from '@/ui-kit/contract'
import { WithHelpTag } from './controls'
import { glyph } from './glyph'
import { Icon as Glyph } from './icons'
import { Surface } from './root'
import { swatchColor } from './swatch'

/**
 * Adaptive window layout: a full-height sidebar (`--sidebar`) beside the content, whose header bar
 * runs across the list and detail panes. The sidebar toggle leads the header bar, so it stays in
 * place while the sidebar slides away. Below the narrow breakpoint (see adwaita.css) the sidebar
 * overlays the content on a scrim instead of squeezing it; the caller's `sidebarHidden` still
 * decides whether it is open.
 */
export function SplitView({
  sidebar,
  sidebarHidden,
  sidebarToggle,
  title,
  subtitle,
  search,
  toolbarLeading,
  toolbar,
  children,
}: SplitViewProps) {
  const toggle = (
    <WithHelpTag
      text={
        sidebarToggle.shortcutHint
          ? `${sidebarToggle.label}  ${sidebarToggle.shortcutHint}`
          : sidebarToggle.label
      }
    >
      <button
        type="button"
        className="adw-btn adw-split-toggle"
        data-size="medium"
        data-tint="borderless"
        data-icon-only=""
        aria-label={sidebarToggle.label}
        aria-expanded={!sidebarHidden}
        onClick={sidebarToggle.onClick}
      >
        <Glyph name={glyph('sidebar')} size={16} />
      </button>
    </WithHelpTag>
  )
  return (
    <div className="adw-split" data-sidebar-hidden={sidebarHidden ? '' : undefined}>
      <div className="adw-split-sidebar" inert={sidebarHidden} aria-hidden={sidebarHidden}>
        <Surface region="sidebar">
          <div className="adw-split-sidebar-inner">{sidebar}</div>
        </Surface>
      </div>
      {/* Only shown below the narrow breakpoint, where the open sidebar overlays the content. */}
      {!sidebarHidden && (
        <button
          type="button"
          className="adw-split-scrim"
          tabIndex={-1}
          aria-label={sidebarToggle.label}
          onClick={sidebarToggle.onClick}
        />
      )}
      <div className="adw-split-content">
        <Surface region="content">
          <header className="adw-split-header">
            {toggle}
            {toolbarLeading}
            <span className="adw-split-heading">
              <span className="adw-split-title">{title}</span>
              {subtitle && <span className="adw-split-subtitle">{subtitle}</span>}
            </span>
            {search}
            {toolbar}
          </header>
          <div className="adw-split-panes">{children}</div>
        </Surface>
      </div>
    </div>
  )
}

export function Pane({ kind, label, children }: PaneProps) {
  return (
    <section className="adw-pane" data-pane={kind} aria-label={label}>
      {children}
    </section>
  )
}

export function Text({
  role = 'body',
  tone = 'primary',
  emphasized,
  monospace,
  large,
  truncate,
  children,
}: TextProps) {
  return (
    <span
      className="adw-text"
      data-role={role}
      data-tone={tone}
      data-emphasized={emphasized ? '' : undefined}
      data-mono={monospace ? '' : undefined}
      data-large={large ? '' : undefined}
      data-truncate={truncate ? '' : undefined}
    >
      {children}
    </span>
  )
}

export function KindBadge({ kind, label, variant, imageSrc }: KindBadgeProps) {
  if (variant === 'tile' && kind === 'image') {
    return (
      <span className="adw-thumb" data-kind="image" role="img" aria-label={label}>
        {imageSrc && <img className="adw-thumb-image" src={imageSrc} alt="" draggable={false} />}
      </span>
    )
  }
  return (
    <span className={variant === 'tile' ? 'adw-thumb' : 'adw-kind-pill'} data-kind={kind}>
      {label}
    </span>
  )
}

/** Bar of a pane's main actions along the bottom edge of the pane. */
export function ActionBar({ children, hint }: ActionBarProps) {
  return (
    <div className="adw-actionbar adw-pane-actionbar">
      {children}
      {hint && <span className="adw-actionbar-hint">{hint}</span>}
    </div>
  )
}

/** AdwStatusPage: large symbol, title, one explanatory line and optional choices. */
export function ContentUnavailable({
  icon,
  title,
  description,
  children,
}: ContentUnavailableProps) {
  return (
    <div className="adw-unavailable" role="status">
      {icon && (
        <span className="adw-unavailable-icon">
          <Glyph name={glyph(icon)} size={48} strokeWidth={1.4} />
        </span>
      )}
      <span className="adw-unavailable-title">{title}</span>
      {description && <span className="adw-unavailable-text">{description}</span>}
      {children && <div className="adw-unavailable-actions">{children}</div>}
    </div>
  )
}

/** Boxed list with a status dot per row. */
export function InsetList({ label, rows }: InsetListProps) {
  return (
    <ul className="adw-grouped" aria-label={label}>
      {rows.map(row => (
        <li key={row.id} data-tone={row.tone ?? 'neutral'}>
          <span className="adw-status-dot" />
          <span className="adw-grouped-title">{row.title}</span>
          {row.detail && <span className="adw-status">{row.detail}</span>}
          {row.action && (
            <button type="button" className="adw-grouped-action" onClick={row.action.onClick}>
              {row.action.label}
            </button>
          )}
        </li>
      ))}
    </ul>
  )
}

export function Icon({ name, label, tone }: IconProps) {
  return (
    <span
      className="adw-inline-icon"
      data-tone={tone}
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <Glyph name={glyph(name)} size={14} />
    </span>
  )
}

export function ColorDot({ color, label }: ColorDotProps) {
  return (
    <span
      className="adw-dot-tag adw-color-dot"
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      style={{ background: color ? swatchColor(color) : 'var(--gray)' }}
    />
  )
}
