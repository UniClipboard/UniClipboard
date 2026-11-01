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
import { useSinglePaneLayout } from '@/ui-kit/skins/linux/adaptive-layout'
import { PaneNavigation, usePaneNavigation } from '@/ui-kit/skins/linux/pane-navigation'
import { SplitSidebarToggle } from '@/ui-kit/skins/linux/split-sidebar-toggle'
import { useSplitNavigation } from '@/ui-kit/use-split-navigation'
import { glyph } from './glyph'
import { Icon as Glyph } from './icons'
import { Surface } from './root'
import { swatchColor } from './swatch'

/**
 * Adaptive window layout: a full-height sidebar (`--sidebar`) beside the content, whose header bar
 * runs across the list and detail panes. The sidebar toggle leads the header bar, so it stays in
 * place while the sidebar slides away. The layout owner chooses an inline or
 * overlay sidebar. In a narrow history window, the list and detail share one content region.
 */
export function SplitView({
  sidebar,
  sidebarHidden,
  sidebarOverlay,
  sidebarToggle,
  title,
  subtitle,
  search,
  toolbarLeading,
  toolbar,
  detailNavigation,
  children,
}: SplitViewProps) {
  const {
    root,
    sidebarRef,
    toggleRef,
    backRef,
    single,
    detail,
    overlay,
    drawerOpen,
    onSidebarKeyDown,
    onSearchFocus,
  } = useSplitNavigation({
    sidebarHidden,
    sidebarOverlay,
    sidebarToggle,
    detailNavigation,
    singlePane: useSinglePaneLayout(),
  })
  return (
    <div
      ref={root}
      className="adw-split"
      data-sidebar-hidden={sidebarHidden ? '' : undefined}
      data-sidebar-overlay={overlay ? '' : undefined}
      data-single-pane={single ? '' : undefined}
      data-detail-open={detail ? '' : undefined}
    >
      <div
        ref={sidebarRef}
        className="adw-split-sidebar"
        inert={sidebarHidden}
        aria-hidden={sidebarHidden}
        role={drawerOpen ? 'dialog' : undefined}
        aria-modal={drawerOpen || undefined}
        aria-label={drawerOpen ? sidebarToggle?.label : undefined}
        onKeyDown={onSidebarKeyDown}
      >
        <Surface region="sidebar">
          <div className="adw-split-sidebar-inner">{sidebar}</div>
        </Surface>
      </div>
      {/* Keep the scrim mounted so it fades with the sidebar in both directions. */}
      {overlay && sidebarToggle && (
        <button
          type="button"
          className="adw-split-scrim"
          inert={!drawerOpen}
          aria-hidden={!drawerOpen}
          tabIndex={-1}
          aria-label={sidebarToggle.label}
          onClick={sidebarToggle.onClick}
        />
      )}
      <div className="adw-split-content" inert={drawerOpen} aria-hidden={drawerOpen || undefined}>
        <Surface region="content">
          <header className="adw-split-toolbar">
            {detail && detailNavigation && (
              <button
                ref={backRef}
                type="button"
                className="adw-btn adw-split-back"
                data-tint="borderless"
                onClick={() => detailNavigation.onOpenChange(false)}
              >
                <Glyph name={glyph('back')} size={16} />
                {detailNavigation.backLabel}
              </button>
            )}
            <SplitSidebarToggle
              action={sidebarToggle}
              hidden={sidebarHidden}
              buttonRef={toggleRef}
            />
            {toolbarLeading}
            <span className="adw-split-heading">
              <span className="adw-split-title">{title}</span>
              {subtitle && <span className="adw-split-subtitle">{subtitle}</span>}
            </span>
            <div className="adw-split-search" onFocusCapture={onSearchFocus}>
              {search}
            </div>
            {toolbar}
          </header>
          <PaneNavigation value={detailNavigation ? { single, detail } : null}>
            <div className="adw-split-panes">{children}</div>
          </PaneNavigation>
        </Surface>
      </div>
    </div>
  )
}

export function Pane({ kind, label, children }: PaneProps) {
  const navigation = usePaneNavigation()
  const hidden = navigation?.single && (kind === 'detail' ? !navigation.detail : navigation.detail)
  return (
    <section
      className="adw-pane"
      data-pane={kind}
      data-pane-hidden={hidden ? '' : undefined}
      inert={hidden}
      aria-hidden={hidden || undefined}
      aria-label={label}
    >
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
          {!row.status && <span className="adw-status-dot" />}
          <span className="adw-grouped-title">{row.title}</span>
          {row.status ?? (row.detail && <span className="adw-status">{row.detail}</span>)}
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
