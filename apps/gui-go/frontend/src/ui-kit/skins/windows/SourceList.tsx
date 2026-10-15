import { makeStyles, mergeClasses, Spinner, tokens } from '@fluentui/react-components'
import { createElement, useRef, useState, type KeyboardEvent } from 'react'
import type { SourceListItem, SourceListProps } from '@/ui-kit/contract'
import { Button } from './Button'
import { iconFor } from './icons'
import { useListStyles } from './list-styles'
import { dotClass, textClass, useStatusStyles } from './status-styles'

// WinUI ListView with group headers inside a management dialog: Body Strong group headers with an
// optional subtle command, rows with an icon on a card tile carrying a status dot, and the accent
// selection pill of a ListViewItem.
const useStyles = makeStyles({
  root: { display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 },
  header: { padding: '0.75rem 0.75rem 0' },
  scroll: {
    flexGrow: 1,
    minHeight: 0,
    overflowY: 'auto',
    padding: '0.25rem 0.5rem 0.5rem',
    display: 'flex',
    flexDirection: 'column',
    gap: '0.125rem',
    outline: 'none',
  },
  heading: {
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: '0.25rem',
    minHeight: '2rem',
    marginTop: '0.5rem',
    paddingInline: '0.75rem 0.25rem',
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    fontWeight: tokens.fontWeightSemibold,
  },
  headingTitle: { flexGrow: 1 },
  row: {
    flexShrink: 0,
    minHeight: '3.25rem',
    ':focus-visible': { outline: `2px solid ${tokens.colorStrokeFocus2}` },
  },
  tile: {
    position: 'relative',
    flexShrink: 0,
    width: '2rem',
    height: '2rem',
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    fontSize: '1rem',
    color: tokens.colorNeutralForeground2,
  },
  pending: {
    backgroundColor: 'transparent',
    border: `${tokens.strokeWidthThin} dashed ${tokens.colorNeutralStroke1}`,
    color: tokens.colorBrandForeground1,
  },
  dot: {
    position: 'absolute',
    insetInlineEnd: '-0.1875rem',
    bottom: '-0.1875rem',
    boxShadow: `0 0 0 2px ${tokens.colorNeutralBackground1}`,
  },
  dimmed: { opacity: 0.55 },
  placeholder: { padding: '0.25rem 0.75rem' },
  extra: {
    display: 'flex',
    flexDirection: 'column',
    gap: '0.375rem',
    paddingBlock: '0.125rem 0.25rem',
  },
  footer: {
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: '0.5rem',
    minHeight: '2.25rem',
    paddingInline: '1rem',
    borderTop: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground2,
  },
})

/**
 * Sectioned object list with one tab stop: Up/Down move the selection across the sections, as the
 * arrow keys of a single-selection ListView do.
 */
export function SourceList({
  label,
  sections,
  value,
  onValueChange,
  header,
  footer,
}: SourceListProps) {
  const styles = useStyles()
  const list = useListStyles()
  const status = useStatusStyles()
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
    const index = flat.findIndex(item => item.id === tabStop)
    const next =
      event.key === 'ArrowDown'
        ? Math.min(flat.length - 1, index + 1)
        : event.key === 'ArrowUp'
          ? Math.max(0, index - 1)
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? flat.length - 1
              : null
    if (next === null || next < 0) return
    event.preventDefault()
    onValueChange(flat[next].id)
    listRef.current?.querySelectorAll<HTMLElement>('[role="option"]')[next]?.focus()
  }

  const row = (item: SourceListItem) => (
    <div
      key={item.id}
      data-testid={item['data-testid']}
      data-status={item['data-status']}
      className={mergeClasses(list.row, styles.row)}
      role="option"
      aria-selected={item.id === value}
      aria-label={item.subtitle ? `${item.title}, ${item.subtitle}` : item.title}
      tabIndex={item.id === tabStop ? 0 : -1}
      onFocus={() => setFocusedId(item.id)}
      onClick={() => onValueChange(item.id)}
    >
      <span
        className={mergeClasses(
          styles.tile,
          item.pending && styles.pending,
          item.dimmed && styles.dimmed
        )}
      >
        {createElement(iconFor(item.icon))}
        {item.status && (
          <span className={mergeClasses(status.dot, status[dotClass[item.status]], styles.dot)} />
        )}
      </span>
      <span className={mergeClasses(list.text, item.dimmed && styles.dimmed)}>
        <span className={list.title}>{item.title}</span>
        {item.subtitle && (
          <span
            className={mergeClasses(
              list.subtitle,
              item.subtitleTone && status[textClass[item.subtitleTone]]
            )}
          >
            {item.subtitle}
          </span>
        )}
      </span>
    </div>
  )

  return (
    <div className={styles.root}>
      {header && <div className={styles.header}>{header}</div>}
      <div
        ref={listRef}
        className={styles.scroll}
        role="listbox"
        aria-label={label}
        onKeyDown={onKeyDown}
      >
        {sections.map(section => (
          <div
            key={section.id}
            role="group"
            aria-label={section.title}
            style={{ display: 'contents' }}
          >
            {section.title && (
              <div className={styles.heading}>
                <span className={styles.headingTitle}>{section.title}</span>
                {section.action &&
                  (section.action.busy ? (
                    <Spinner size="extra-tiny" aria-label={section.action.label} />
                  ) : (
                    <Button
                      variant="subtle"
                      size="small"
                      icon={section.action.icon}
                      label={section.action.label}
                      disabled={section.action.disabled}
                      onClick={section.action.onClick}
                    />
                  ))}
              </div>
            )}
            {section.items.length === 0 && section.placeholder ? (
              <div className={styles.placeholder}>
                <Button size="small" icon="add" onClick={section.placeholder.onClick}>
                  {section.placeholder.label}
                </Button>
              </div>
            ) : (
              section.items.map(row)
            )}
            {section.extra && <div className={styles.extra}>{section.extra}</div>}
          </div>
        ))}
      </div>
      {footer && (
        <div className={styles.footer} role="status">
          <span className={mergeClasses(status.dot, status.dotPositive)} />
          {footer}
        </div>
      )}
    </div>
  )
}
