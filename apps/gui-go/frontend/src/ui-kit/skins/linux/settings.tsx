import type { SettingsContentProps, SettingsGroupProps, SettingsRowProps } from '@/ui-kit/contract'
import './settings.css'

/**
 * Scrolling body of a settings category (AdwPreferencesPage): a centred column of boxed groups on
 * the window background. The scroll element carries `--adw-accent` so the column below it can give
 * the app's own (shadcn) controls the system accent without losing the kit's.
 */
export function SettingsContent({ label, viewportRef, children }: SettingsContentProps) {
  return (
    <div ref={viewportRef} className="adw-settings-scroll" role="region" aria-label={label}>
      <div className="adw-settings-column">{children}</div>
    </div>
  )
}

/** AdwPreferencesGroup: a bold heading above a boxed list of rows. */
export function SettingsGroup({ title, children }: SettingsGroupProps) {
  return (
    <fieldset className="adw-settings-group">
      {title && <legend className="adw-settings-group-title">{title}</legend>}
      <div className="adw-settings-group-body">{children}</div>
    </fieldset>
  )
}

/** AdwActionRow: title and subtitle on the leading side, the control on the trailing side. */
export function SettingsRow({ label, labelExtra, description, children }: SettingsRowProps) {
  const hasText = Boolean(label || description)
  return (
    <div className="adw-settings-row" data-bare={hasText ? undefined : ''}>
      {hasText && (
        <div className="adw-settings-row-text">
          {label && (
            <span className="adw-settings-row-label">
              {label}
              {labelExtra}
            </span>
          )}
          {description && <span className="adw-settings-row-description">{description}</span>}
        </div>
      )}
      {children && <div className="adw-settings-row-control">{children}</div>}
    </div>
  )
}
