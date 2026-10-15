import type { EntityHeaderProps, MasterDetailProps } from '@/ui-kit/contract'
import { glyph } from './glyph'
import { Icon } from './icons'

/**
 * The two columns of a large sheet (design DV1): a fixed source-list column with a hairline
 * divider and a detail column that scrolls by itself, as in GNOME Settings.
 */
export function MasterDetail({ list, detail, detailLabel }: MasterDetailProps) {
  return (
    <div className="adw-master-detail">
      <div className="adw-master-column">{list}</div>
      <section className="adw-detail-column" aria-label={detailLabel}>
        <div className="adw-detail-column-body">{detail}</div>
      </section>
    </div>
  )
}

/** Detail heading (design DV1): a 56pt icon tile, the name, a status line and trailing commands. */
export function EntityHeader({
  icon,
  pending,
  title,
  badge,
  status,
  description,
  actions,
}: EntityHeaderProps) {
  return (
    <div className="adw-entity-header">
      <span className="adw-entity-tile" data-pending={pending ? '' : undefined}>
        <Icon name={glyph(icon)} size={30} strokeWidth={1.6} />
      </span>
      <div className="adw-entity-text">
        <span className="adw-entity-title-row">
          <h2 className="adw-entity-title" title={title}>
            {title}
          </h2>
          {badge && <span className="adw-entity-badge">{badge}</span>}
        </span>
        {status && (
          <span className="adw-entity-status" data-tone={status.tone}>
            <span className="adw-source-dot" data-status={status.tone} />
            <span className="adw-entity-status-text">{status.text}</span>
            {status.detail && (
              <>
                <span aria-hidden="true">·</span>
                <span>{status.detail}</span>
              </>
            )}
          </span>
        )}
        {description && <span className="adw-entity-description">{description}</span>}
      </div>
      {actions && <div className="adw-entity-actions">{actions}</div>}
    </div>
  )
}
