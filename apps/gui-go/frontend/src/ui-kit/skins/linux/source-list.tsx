import { useRef, useState, type KeyboardEvent } from 'react'
import type { SourceListItem, SourceListProps } from '@/ui-kit/contract'
import { WithHelpTag } from './controls'
import { glyph } from './glyph'
import { Icon } from './icons'
import { listStep } from './list-keys'
import { PushButton } from './parts'
import { Spinner } from './spinner'

/**
 * Source list of a management sheet (design DV1): small bold section headings with an optional
 * icon command, rows with an icon tile carrying a status dot, and a blue selection. One tab stop
 * for all rows; Up/Down move the selection across sections, as in an GTK list.
 */
export function SourceList({
  label,
  sections,
  value,
  onValueChange,
  header,
  footer,
}: SourceListProps) {
  const listRef = useRef<HTMLDivElement>(null)
  const [focusedId, setFocusedId] = useState<string | undefined>()
  const flat = sections.flatMap(section => section.items)
  const tabStop =
    flat.find(item => item.id === focusedId)?.id ??
    flat.find(item => item.id === value)?.id ??
    flat[0]?.id

  const onKeyDown = (event: KeyboardEvent) => {
    if (!(event.target instanceof HTMLElement) || event.target.getAttribute('role') !== 'option')
      return
    const next = listStep(
      event.key,
      flat.findIndex(item => item.id === tabStop),
      flat.length
    )
    if (next === null) return
    event.preventDefault()
    onValueChange(flat[next].id)
    listRef.current?.querySelectorAll<HTMLElement>('[role="option"]')[next]?.focus()
  }

  const row = (item: SourceListItem) => (
    <div
      key={item.id}
      data-testid={item['data-testid']}
      data-status={item['data-status']}
      className="adw-source-row"
      role="option"
      aria-selected={item.id === value}
      aria-label={item.subtitle ? `${item.title}, ${item.subtitle}` : item.title}
      tabIndex={item.id === tabStop ? 0 : -1}
      data-dimmed={item.dimmed ? '' : undefined}
      onFocus={() => setFocusedId(item.id)}
      onClick={() => onValueChange(item.id)}
    >
      <span className="adw-source-tile" data-pending={item.pending ? '' : undefined}>
        <Icon name={glyph(item.icon)} size={16} />
        {item.status && <span className="adw-source-dot" data-status={item.status} />}
      </span>
      <span className="adw-row-text">
        <span className="adw-source-title">{item.title}</span>
        {item.subtitle && (
          <span className="adw-source-subtitle" data-tone={item.subtitleTone}>
            {item.subtitle}
          </span>
        )}
      </span>
    </div>
  )

  return (
    <div className="adw-source-list">
      {header && <div className="adw-source-header">{header}</div>}
      <div
        ref={listRef}
        className="adw-source-scroll"
        role="listbox"
        aria-label={label}
        onKeyDown={onKeyDown}
      >
        {sections.map(section => (
          <div
            key={section.id}
            role="group"
            aria-label={section.title}
            className="adw-source-group"
          >
            {section.title && (
              <div className="adw-source-heading">
                <span className="adw-side-heading-title">{section.title}</span>
                {section.action && (
                  <WithHelpTag text={section.action.label}>
                    <button
                      type="button"
                      className="adw-side-heading-action"
                      aria-label={section.action.label}
                      aria-busy={section.action.busy || undefined}
                      disabled={section.action.disabled || section.action.busy}
                      onClick={section.action.onClick}
                    >
                      {section.action.busy ? (
                        <Spinner size={13} />
                      ) : (
                        <Icon name={glyph(section.action.icon)} size={13} strokeWidth={1.9} />
                      )}
                    </button>
                  </WithHelpTag>
                )}
              </div>
            )}
            {section.items.length === 0 && section.placeholder ? (
              <div className="adw-source-placeholder">
                <PushButton icon="plus" onClick={section.placeholder.onClick}>
                  {section.placeholder.label}
                </PushButton>
              </div>
            ) : (
              section.items.map(row)
            )}
            {section.extra && <div className="adw-source-extra">{section.extra}</div>}
          </div>
        ))}
      </div>
      {footer && (
        <div className="adw-source-footer" role="status">
          {footer}
        </div>
      )}
    </div>
  )
}
