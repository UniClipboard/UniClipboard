import type { SettingsContentProps, SettingsGroupProps, SettingsRowProps } from '@/ui-kit/contract'
import './settings.css'

/**
 * Scrolling body of a settings category (design MS1): a centred column of inset groups under the
 * unified toolbar band. The scroll element carries `--mac-accent` so the column below it can give
 * the app's own (shadcn) controls the system accent without losing the kit's.
 */
export function SettingsContent({ label, viewportRef, children }: SettingsContentProps) {
  return (
    <div ref={viewportRef} className="mac-settings-scroll" role="region" aria-label={label}>
      <div className="mac-settings-column">{children}</div>
    </div>
  )
}

/** Inset group: small bold heading above a rounded, separator-divided list of rows. */
export function SettingsGroup({ title, children }: SettingsGroupProps) {
  return (
    <fieldset className="mac-settings-group">
      {title && <legend className="mac-settings-group-title">{title}</legend>}
      <div className="mac-settings-group-body">{children}</div>
    </fieldset>
  )
}

export function SettingsRow({ label, labelExtra, description, children }: SettingsRowProps) {
  const hasText = Boolean(label || description)
  return (
    <div className="mac-settings-row" data-bare={hasText ? undefined : ''}>
      {hasText && (
        <div className="mac-settings-row-text">
          {label && (
            <span className="mac-settings-row-label">
              {label}
              {labelExtra}
            </span>
          )}
          {description && <span className="mac-settings-row-description">{description}</span>}
        </div>
      )}
      {children && <div className="mac-settings-row-control">{children}</div>}
    </div>
  )
}
