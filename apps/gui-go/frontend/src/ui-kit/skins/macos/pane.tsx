import type {
  ActionBarProps,
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

/**
 * NSSplitViewController window: a full-height source-list sidebar on the window material, and the
 * content with one unified toolbar band on top. The traffic lights sit at the top of the sidebar;
 * with the sidebar hidden they sit at the leading end of the toolbar, followed by the sidebar
 * button (macOS collapses the sidebar completely, there is no icon rail).
 */
export function SplitView({
  sidebar,
  sidebarHidden,
  sidebarToggle,
  title,
  subtitle,
  search,
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
        className="mac-split-toggle"
        aria-label={sidebarToggle.label}
        aria-expanded={!sidebarHidden}
        onClick={sidebarToggle.onClick}
      >
        <Glyph name={glyph('sidebar')} size={17} strokeWidth={1.6} />
      </button>
    </WithHelpTag>
  )
  return (
    <div className="mac-split" data-sidebar-hidden={sidebarHidden ? '' : undefined}>
      {!sidebarHidden && (
        <div className="mac-split-sidebar">
          <Surface region="sidebar">
            <div className="mac-split-sidebar-inner">
              <div className="mac-split-chrome" data-window-drag="">
                {toggle}
              </div>
              {sidebar}
            </div>
          </Surface>
        </div>
      )}
      <div className="mac-split-content">
        <Surface region="content">
          <div className="mac-split-panes">{children}</div>
          <div className="mac-split-scroll-edge" aria-hidden="true" />
          <div className="mac-split-toolbar" data-window-drag="">
            {sidebarHidden && toggle}
            <span className="mac-split-title">{title}</span>
            {subtitle && <span className="mac-split-subtitle">{subtitle}</span>}
            <span className="mac-split-spacer" />
            {search}
            {toolbar}
          </div>
        </Surface>
      </div>
    </div>
  )
}

export function Pane({ kind, label, children }: PaneProps) {
  return (
    <section className="mac-pane" data-pane={kind} aria-label={label}>
      {children}
    </section>
  )
}

export function Text({
  role = 'body',
  tone = 'primary',
  emphasized,
  monospace,
  truncate,
  children,
}: TextProps) {
  return (
    <span
      className="mac-text"
      data-role={role}
      data-tone={tone}
      data-emphasized={emphasized ? '' : undefined}
      data-mono={monospace ? '' : undefined}
      data-truncate={truncate ? '' : undefined}
    >
      {children}
    </span>
  )
}

export function KindBadge({ kind, label, variant, imageSrc }: KindBadgeProps) {
  if (variant === 'tile' && kind === 'image') {
    return (
      <span className="mac-thumb" data-kind="image" role="img" aria-label={label}>
        {imageSrc && <img className="mac-thumb-image" src={imageSrc} alt="" draggable={false} />}
      </span>
    )
  }
  return (
    <span className={variant === 'tile' ? 'mac-thumb' : 'mac-kind-pill'} data-kind={kind}>
      {label}
    </span>
  )
}

/** Floating Liquid Glass bar of a pane's main actions (design MA2 "glass action bar"). */
export function ActionBar({ children, hint }: ActionBarProps) {
  return (
    <div className="mac-actionbar mac-glass mac-pane-actionbar">
      {children}
      {hint && <span className="mac-actionbar-hint">{hint}</span>}
    </div>
  )
}

/** NSContentUnavailableView: large symbol, title, one explanatory line and optional choices. */
export function ContentUnavailable({
  icon,
  title,
  description,
  children,
}: ContentUnavailableProps) {
  return (
    <div className="mac-unavailable" role="status">
      {icon && (
        <span className="mac-unavailable-icon">
          <Glyph name={glyph(icon)} size={34} strokeWidth={1.4} />
        </span>
      )}
      <span className="mac-unavailable-title">{title}</span>
      {description && <span className="mac-unavailable-text">{description}</span>}
      {children && <div className="mac-unavailable-actions">{children}</div>}
    </div>
  )
}

/** Inset grouped table with a status dot per row. */
export function InsetList({ label, rows }: InsetListProps) {
  return (
    <ul className="mac-grouped" aria-label={label}>
      {rows.map(row => (
        <li key={row.id} data-tone={row.tone ?? 'neutral'}>
          <span className="mac-status-dot" />
          <span className="mac-grouped-title">{row.title}</span>
          {row.detail && <span className="mac-status">{row.detail}</span>}
          {row.action && (
            <button type="button" className="mac-grouped-action" onClick={row.action.onClick}>
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
      className="mac-inline-icon"
      data-tone={tone}
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <Glyph name={glyph(name)} size={12} strokeWidth={1.6} />
    </span>
  )
}
