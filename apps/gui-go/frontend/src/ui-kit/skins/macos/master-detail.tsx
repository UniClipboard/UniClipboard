import type { EntityHeaderProps, MasterDetailProps } from '@/ui-kit/contract'
import { glyph } from './glyph'
import { Icon } from './icons'

/**
 * The two columns of a large sheet (design DV1): a fixed source-list column with a hairline
 * divider and a detail column that scrolls by itself, as in System Settings.
 */
export function MasterDetail({ list, detail, detailKey, detailLabel }: MasterDetailProps) {
  return (
    <div className="mac-master-detail">
      <div className="mac-master-column">{list}</div>
      <section key={detailKey} className="mac-detail-column" aria-label={detailLabel}>
        <div className="mac-detail-column-body">{detail}</div>
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
    <div className="mac-entity-header">
      <span className="mac-entity-tile" data-pending={pending ? '' : undefined}>
        <Icon name={glyph(icon)} size={30} strokeWidth={1.5} />
      </span>
      <div className="mac-entity-text">
        <span className="mac-entity-title-row">
          <h2 className="mac-entity-title" title={title}>
            {title}
          </h2>
          {badge && <span className="mac-entity-badge">{badge}</span>}
        </span>
        {status && (
          <span className="mac-entity-status" data-tone={status.tone}>
            <span className="mac-source-dot" data-status={status.tone} />
            <span className="mac-entity-status-text">{status.text}</span>
            {status.detail && (
              <>
                <span aria-hidden="true">·</span>
                <span>{status.detail}</span>
              </>
            )}
          </span>
        )}
        {description && <span className="mac-entity-description">{description}</span>}
      </div>
      {actions && <div className="mac-entity-actions">{actions}</div>}
    </div>
  )
}
