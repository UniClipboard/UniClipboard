import {
  Menu as FluentMenu,
  MenuPopover,
  MenuTrigger,
  makeStyles,
  mergeClasses,
  tokens,
} from '@fluentui/react-components'
import { CheckmarkFilled } from '@fluentui/react-icons'
import { useEffect, useId, useRef, useState } from 'react'
import type { KeyboardEvent, MouseEvent } from 'react'
import type { GridItem, GroupedGridProps } from '@/ui-kit/contract'
import { selectRange } from './list-keys'
import { useMenuStyles } from './menu-styles'
import { MenuEntries } from './MenuEntries'

// WinUI GridView cards on the card layer (boards C3/C4 vocabulary): an image, a dark code snippet, a link
// (host and address) or text in the card body, title and subtitle below; selection draws the accent border
// and the check in the corner. Card width follows `size` (0..1) between 10 and 16 rem.
const useStyles = makeStyles({
  menuArea: { flexGrow: 1, minHeight: 0, display: 'flex', flexDirection: 'column' },
  grid: {
    flexGrow: 1,
    minHeight: 0,
    overflowY: 'auto',
    outline: 'none',
    paddingInline: '1rem',
    paddingBottom: '1rem',
    '&:focus-visible [data-cursor]': {
      outline: `${tokens.strokeWidthThick} solid ${tokens.colorStrokeFocus2}`,
      outlineOffset: '2px',
    },
  },
  header: {
    position: 'sticky',
    top: 0,
    zIndex: 1,
    display: 'flex',
    alignItems: 'baseline',
    gap: '0.5rem',
    paddingBlock: '0.75rem 0.5rem',
    backgroundColor: 'var(--w11-card-solid)',
  },
  headerTitle: { fontSize: tokens.fontSizeBase300, fontWeight: tokens.fontWeightSemibold },
  headerDetail: { fontSize: tokens.fontSizeBase200, color: tokens.colorNeutralForeground2 },
  headerTrailing: {
    marginInlineStart: 'auto',
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground3,
  },
  cards: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fill, minmax(var(--w11-card-width), 1fr))',
    gap: '0.75rem',
  },
  card: {
    position: 'relative',
    display: 'flex',
    flexDirection: 'column',
    minWidth: 0,
    borderRadius: tokens.borderRadiusXLarge,
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    cursor: 'default',
    userSelect: 'none',
    overflow: 'hidden',
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
    '&[aria-selected="true"]': {
      boxShadow: `0 0 0 2px ${tokens.colorBrandBackground}`,
      borderTopColor: 'transparent',
      borderRightColor: 'transparent',
      borderBottomColor: 'transparent',
      borderLeftColor: 'transparent',
    },
  },
  body: {
    aspectRatio: '4 / 3',
    minHeight: 0,
    overflow: 'hidden',
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'flex-start',
    padding: '0.625rem',
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
    wordBreak: 'break-word',
  },
  media: {
    padding: 0,
    alignItems: 'stretch',
    '& > *': { width: '100%', height: '100%', objectFit: 'cover' },
    '& img': { width: '100%', height: '100%', objectFit: 'cover' },
  },
  code: {
    backgroundColor: 'var(--w11-code-bg)',
    color: '#D4D4D4',
    fontFamily: tokens.fontFamilyMonospace,
    whiteSpace: 'pre-wrap',
  },
  link: { color: tokens.colorBrandForeground1 },
  meta: { padding: '0.5rem 0.625rem 0.625rem', minWidth: 0 },
  title: {
    fontSize: tokens.fontSizeBase300,
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  subtitle: {
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground2,
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  badges: {
    position: 'absolute',
    top: '0.375rem',
    insetInlineStart: '0.375rem',
    display: 'flex',
    gap: '0.25rem',
  },
  check: {
    position: 'absolute',
    top: '0.375rem',
    insetInlineEnd: '0.375rem',
    width: '1.25rem',
    height: '1.25rem',
    borderRadius: tokens.borderRadiusMedium,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: tokens.colorBrandBackground,
    color: tokens.colorNeutralForegroundOnBrand,
  },
  footer: {
    paddingBlock: '0.75rem',
    display: 'flex',
    justifyContent: 'center',
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground2,
  },
})

/** The card body by kind: media for an image, a code snippet, a link's text, or plain text. */
function CardBody({ item, styles }: { item: GridItem; styles: ReturnType<typeof useStyles> }) {
  if (item.media) return <div className={mergeClasses(styles.body, styles.media)}>{item.media}</div>
  return (
    <div
      className={mergeClasses(
        styles.body,
        item.kind === 'code' && styles.code,
        item.kind === 'link' && styles.link
      )}
    >
      {item.text}
    </div>
  )
}

/**
 * Keyboard and pointer as GroupedList, with arrows in two dimensions: Left/Right step one card, Up/Down one
 * row of the current group's columns. The grid is one tab stop naming the current card with
 * aria-activedescendant; every card carries `data-item-id`. Cards are all rendered (no virtualization):
 * `onEndReached` fires when the end of the loaded cards scrolls into view.
 */
export function GroupedGrid({
  label,
  groups,
  items,
  size,
  selection,
  onSelectionChange,
  onActivate,
  onDelete,
  onEndReached,
  contextMenu,
  footer,
}: GroupedGridProps) {
  const styles = useStyles()
  const menuStyles = useMenuStyles()
  const domId = useId()
  const gridRef = useRef<HTMLDivElement>(null)
  const endRef = useRef<HTMLDivElement>(null)
  const anchor = useRef<string | null>(null)
  const pendingMenuIds = useRef<string[]>([])
  const [menuIds, setMenuIds] = useState<string[] | null>(null)
  const cursor = selection[selection.length - 1]
  const cardId = (id: string) => `${domId}-card-${id}`
  const endReached = useRef(onEndReached)
  endReached.current = onEndReached

  // Re-observed after each page: an end marker that stays in view reports no new intersection.
  useEffect(() => {
    const end = endRef.current
    const root = gridRef.current
    if (!end || !root) return
    const observer = new IntersectionObserver(
      entries => {
        if (entries.some(entry => entry.isIntersecting)) endReached.current?.()
      },
      { root, rootMargin: '240px' }
    )
    observer.observe(end)
    return () => observer.disconnect()
  }, [items.length])

  const selectOne = (id: string) => {
    anchor.current = id
    onSelectionChange([id])
  }
  const reveal = (id: string) =>
    document.getElementById(cardId(id))?.scrollIntoView({ block: 'nearest' })
  // Columns of the row the cursor is in: cards of its group that share the first card's top.
  const columnsAt = (index: number) => {
    const card = document.getElementById(cardId(items[index].id))
    const row = card?.parentElement
    if (!row) return 1
    const children = [...row.children] as HTMLElement[]
    const top = children[0]?.offsetTop
    return Math.max(1, children.filter(child => child.offsetTop === top).length)
  }
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget || items.length === 0) return
    const found = items.findIndex(item => item.id === cursor)
    const index = Math.max(0, found)
    const step: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -columnsAt(index),
      ArrowDown: columnsAt(index),
    }
    let next: number | null = null
    // With no current card the first arrow selects the first card, as in GroupedList.
    if (event.key in step)
      next = found < 0 ? 0 : Math.min(items.length - 1, Math.max(0, index + step[event.key]))
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = items.length - 1
    if (next !== null) {
      event.preventDefault()
      const id = items[next].id
      if (event.shiftKey && anchor.current)
        onSelectionChange(selectRange(items, anchor.current, id))
      else selectOne(id)
      reveal(id)
    } else if (event.ctrlKey && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      onSelectionChange(items.map(item => item.id))
    } else if (event.key === 'Enter' && cursor) {
      event.preventDefault()
      onActivate?.(cursor)
    } else if (event.key === 'Delete' && selection.length > 0 && onDelete) {
      event.preventDefault()
      onDelete(selection)
    }
  }
  const onCardMouseDown = (event: MouseEvent, id: string) => {
    event.preventDefault()
    gridRef.current?.focus({ preventScroll: true })
    const selected = selection.includes(id)
    if (event.button === 2) {
      if (!selected) selectOne(id)
      return
    }
    if (event.button !== 0) return
    if (event.ctrlKey) {
      anchor.current = id
      onSelectionChange(selected ? selection.filter(s => s !== id) : [...selection, id])
    } else if (event.shiftKey && anchor.current) {
      onSelectionChange(selectRange(items, anchor.current, id))
    } else {
      selectOne(id)
    }
  }

  let offset = 0
  const sections = groups.map(group => {
    const groupItems = items.slice(offset, offset + group.count)
    offset += group.count
    return { group, groupItems }
  })
  const width = `${10 + Math.min(1, Math.max(0, size)) * 6}rem`

  const grid = (
    <div
      ref={gridRef}
      className={styles.grid}
      role="listbox"
      tabIndex={0}
      aria-label={label}
      aria-multiselectable
      aria-activedescendant={cursor ? cardId(cursor) : undefined}
      onKeyDown={onKeyDown}
      onContextMenuCapture={event => {
        pendingMenuIds.current = event.target === event.currentTarget ? selection : []
      }}
      style={{ '--w11-card-width': width } as React.CSSProperties}
    >
      {sections.map(({ group, groupItems }) => (
        <div key={group.id} role="presentation">
          <div className={styles.header} role="presentation">
            <span className={styles.headerTitle}>{group.title}</span>
            {group.detail ? <span className={styles.headerDetail}>{group.detail}</span> : null}
            {group.trailing ? (
              <span className={styles.headerTrailing}>{group.trailing}</span>
            ) : null}
          </div>
          <div className={styles.cards} role="presentation">
            {groupItems.map(item => {
              const selected = selection.includes(item.id)
              return (
                <div
                  key={item.id}
                  id={cardId(item.id)}
                  role="option"
                  aria-selected={selected}
                  aria-label={item.title}
                  data-item-id={item.id}
                  data-cursor={item.id === cursor ? '' : undefined}
                  className={styles.card}
                  onMouseDown={event => onCardMouseDown(event, item.id)}
                  onDoubleClick={() => onActivate?.(item.id)}
                  onContextMenu={() => {
                    pendingMenuIds.current = selected ? selection : [item.id]
                  }}
                >
                  <CardBody item={item} styles={styles} />
                  <div className={styles.meta}>
                    <div className={styles.title}>{item.title}</div>
                    {item.subtitle ? <div className={styles.subtitle}>{item.subtitle}</div> : null}
                  </div>
                  {item.badges ? <div className={styles.badges}>{item.badges}</div> : null}
                  {selected && selection.length > 1 ? (
                    <span className={styles.check} aria-hidden="true">
                      <CheckmarkFilled fontSize={12} />
                    </span>
                  ) : null}
                </div>
              )
            })}
          </div>
        </div>
      ))}
      <div ref={endRef} role="presentation" />
      {footer ? <div className={styles.footer}>{footer}</div> : null}
    </div>
  )

  if (!contextMenu) return grid
  return (
    <FluentMenu
      openOnContext
      open={menuIds !== null}
      onOpenChange={(_, data) => {
        if (!data.open) {
          setMenuIds(null)
          gridRef.current?.focus({ preventScroll: true })
        } else if (pendingMenuIds.current.length > 0) setMenuIds(pendingMenuIds.current)
      }}
    >
      <MenuTrigger disableButtonEnhancement>
        <div className={styles.menuArea}>{grid}</div>
      </MenuTrigger>
      <MenuPopover className={menuStyles.popover}>
        <MenuEntries
          items={menuIds ? contextMenu.items(menuIds) : []}
          onAction={id => {
            contextMenu.onAction(id, menuIds ?? [])
            gridRef.current?.focus({ preventScroll: true })
          }}
        />
      </MenuPopover>
    </FluentMenu>
  )
}
