import { Toast } from '@base-ui/react/toast'
import { Toolbar } from '@base-ui/react/toolbar'
import { Tooltip } from '@base-ui/react/tooltip'
import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type KeyboardEvent,
} from 'react'
import {
  ActionBar,
  ClipRow,
  CodeCard,
  DeliveryList,
  InlineNotice,
  SidebarRow,
} from '@/prototypes/macos-design/content'
import { clips, sidebarSections, type Clip, type TagName } from '@/prototypes/macos-design/data'
import {
  ClipContextMenu,
  DeleteAlert,
  MoreMenuButton,
  SendToPopover,
  TagSheet,
  ToastStack,
  ToolbarButton,
  type ClipAction,
} from '@/prototypes/macos-design/overlays'
import { listStep } from '@/ui-kit/skins/macos/list-keys'
import { PushButton, SearchField, Tag } from '@/ui-kit/skins/macos/parts'
import { PortalContainer } from '@/ui-kit/skins/macos/portal'

export type Generation = '26' | '27'

/**
 * What is behind the sidebar: the real window material reported by the Wails host, or the CSS
 * approximation. Only `native-*` values may be called Liquid Glass / vibrancy.
 */
export type WindowMaterial = 'native-liquid-glass' | 'native-visual-effect' | 'css-fallback'

function subscribeFocus(onChange: () => void) {
  window.addEventListener('focus', onChange)
  window.addEventListener('blur', onChange)
  return () => {
    window.removeEventListener('focus', onChange)
    window.removeEventListener('blur', onChange)
  }
}

/** Web approximation of "key window": document focus. AppKit's key/main window state is not visible to the page. */
function useDocumentFocused() {
  return useSyncExternalStore(subscribeFocus, () => document.hasFocus())
}

const sidebarFilter: Record<string, (clip: Clip) => boolean> = {
  all: () => true,
  pinned: clip => Boolean(clip.pinned),
  'deploy-commands': clip => clip.tags.includes('deploy'),
  invoices: clip => clip.kind === 'pdf',
  'tag-docker': clip => clip.tags.includes('docker'),
  'tag-deploy': clip => clip.tags.includes('deploy'),
}

const sidebarItems = sidebarSections.flatMap(section => section.items)

export function ClipboardWindow({
  generation,
  width,
  height,
  showDetail = true,
  initialScrollTop = 0,
  initialSelection = ['compose-up'],
  demoHoverId,
  alwaysActive = false,
  shell = 'web',
  material = 'css-fallback',
}: {
  generation: Generation
  width: number | string
  height: number | string
  showDetail?: boolean
  initialScrollTop?: number
  initialSelection?: string[]
  /** Specimen boards show the sidebar hover state without a pointer. */
  demoHoverId?: string
  /** Specimen boards render as the key window even when the page itself is not focused. */
  alwaysActive?: boolean
  shell?: 'web' | 'native'
  material?: WindowMaterial
}) {
  const focused = useDocumentFocused()
  const active = alwaysActive || focused
  const [windowEl, setWindowEl] = useState<HTMLDivElement | null>(null)
  const [section, setSection] = useState('all')
  const [selection, setSelection] = useState<string[]>(initialSelection)
  const [deleted, setDeleted] = useState<string[]>([])
  const [pinned, setPinned] = useState(() => clips.filter(clip => clip.pinned).map(clip => clip.id))
  const [extraTags, setExtraTags] = useState<Record<string, TagName[]>>({})
  const [query, setQuery] = useState('')
  const [sidebarHidden, setSidebarHidden] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [tagOpen, setTagOpen] = useState(false)
  const searchRef = useRef<HTMLInputElement>(null)
  const [toastManager] = useState(() => Toast.createToastManager())

  const items = clips
    .map(clip => ({
      ...clip,
      pinned: pinned.includes(clip.id),
      tags: [...clip.tags, ...(extraTags[clip.id] ?? [])],
    }))
    .filter(clip =>
      section === 'deleted'
        ? deleted.includes(clip.id)
        : !deleted.includes(clip.id) && (sidebarFilter[section]?.(clip) ?? true)
    )
    .filter(clip => clip.title.toLowerCase().includes(query.trim().toLowerCase()))
  const visibleSelection = selection.filter(id => items.some(item => item.id === id))
  const current = items.find(item => item.id === visibleSelection[visibleSelection.length - 1])
  const sectionLabel = sidebarItems.find(item => item.id === section)

  useEffect(() => {
    // ⌘F focuses the toolbar search field from anywhere in the window.
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.metaKey && event.key.toLowerCase() === 'f') {
        event.preventDefault()
        searchRef.current?.focus()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  const notify = (title: string, description: string) =>
    toastManager.add({ title, description, timeout: 2600 })

  const runAction = (action: ClipAction) => {
    if (!current) return
    if (action === 'copy') notify('Copied', current.title)
    if (action === 'send') notify('Sent', 'Arrived on 2 devices.')
    if (action === 'edit') notify('Edit', 'Editing is outside this prototype.')
    if (action === 'tag') setTagOpen(true)
    if (action === 'pin')
      setPinned(list =>
        list.includes(current.id) ? list.filter(id => id !== current.id) : [...list, current.id]
      )
    if (action === 'delete') setDeleteOpen(true)
  }

  const onListKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = items.findIndex(item => item.id === current?.id)
    const next = listStep(event.key, index, items.length)
    if (next !== null) {
      event.preventDefault()
      const id = items[next].id
      setSelection(event.shiftKey ? [...new Set([...visibleSelection, id])] : [id])
      document.getElementById(`clip-${id}`)?.scrollIntoView({ block: 'nearest' })
      return
    }
    if (event.metaKey && event.key === 'a') {
      event.preventDefault()
      setSelection(items.map(item => item.id))
    } else if (event.key === 'Enter') {
      event.preventDefault()
      runAction('copy')
    } else if (event.key === 'Backspace' || event.key === 'Delete') {
      event.preventDefault()
      runAction('delete')
    } else if (event.metaKey && event.key.toLowerCase() === 'p') {
      event.preventDefault()
      runAction('pin')
    } else if (!event.metaKey && event.key.toLowerCase() === 't') {
      event.preventDefault()
      runAction('tag')
    }
  }

  const onSidebarKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = sidebarItems.findIndex(item => item.id === section)
    const next = listStep(event.key, index, sidebarItems.length)
    if (next !== null) {
      event.preventDefault()
      setSection(sidebarItems[next].id)
    }
  }

  const selectRow = (id: string, additive: boolean) =>
    setSelection(list =>
      additive ? (list.includes(id) ? list.filter(item => item !== id) : [...list, id]) : [id]
    )

  const detailWidth = showDetail ? 330 : 0

  return (
    <Toast.Provider toastManager={toastManager} limit={1}>
      <Tooltip.Provider delay={700}>
        <div
          ref={setWindowEl}
          className="mac-window"
          data-generation={generation}
          data-shell={shell}
          data-window-material={material}
          data-window-inactive={active ? undefined : ''}
          data-sidebar-hidden={sidebarHidden ? '' : undefined}
          style={{ width, height, '--detail-width': `${detailWidth}px` } as CSSProperties}
          aria-label="UniClipboard"
          role="group"
        >
          {/* Opaque content layer: Liquid Glass belongs to the navigation layer, content stays solid. */}
          <div className="mac-content-surface" />
          <ClipContextMenu onAction={runAction}>
            <div
              className="mac-list"
              role="listbox"
              aria-label={sectionLabel?.label ?? 'Items'}
              aria-multiselectable="true"
              aria-activedescendant={current ? `clip-${current.id}` : undefined}
              tabIndex={0}
              style={{ right: `calc(var(--detail-width) + ${generation === '27' ? 14 : 16}px)` }}
              ref={element => {
                if (element && initialScrollTop) element.scrollTop = initialScrollTop
              }}
              onKeyDown={onListKeyDown}
            >
              {items.map(clip => (
                <ClipRow
                  key={clip.id}
                  id={`clip-${clip.id}`}
                  clip={clip}
                  selected={visibleSelection.includes(clip.id)}
                  onMouseDown={event => {
                    // Right-click on an unselected row selects it first, as Finder does.
                    if (event.button === 2 && visibleSelection.includes(clip.id)) return
                    selectRow(clip.id, event.metaKey)
                  }}
                  onDoubleClick={() => runAction('copy')}
                />
              ))}
              {items.length === 0 && (
                <p className="mac-row-meta" style={{ padding: '24px 10px' }}>
                  No items
                </p>
              )}
            </div>
          </ClipContextMenu>

          {showDetail && current && (
            <section
              className="mac-detail"
              style={{ width: detailWidth }}
              aria-label="Item details"
            >
              <h2 className="mac-detail-title">{current.title}</h2>
              <span className="mac-row-meta">
                {current.source} · {current.time}
              </span>
              <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
                {current.tags.map(tag => (
                  <Tag key={tag} label={`#${tag}`} color={tag} />
                ))}
                <button type="button" className="mac-tag-add" onClick={() => setTagOpen(true)}>
                  + Add tag
                </button>
              </div>
              {current.body && (
                <CodeCard
                  body={current.body}
                  lines={current.body.split('\n').length}
                  chars={current.body.length}
                />
              )}
              <DeliveryList />
              <InlineNotice
                tone="red"
                title="Not delivered"
                text="Pixel 8 rejected images."
                action={
                  <PushButton size="small" onClick={() => notify('Retrying', 'Pixel 8')}>
                    Retry
                  </PushButton>
                }
              />
              <span style={{ flexGrow: 1 }} />
              <ActionBar
                onCopy={() => runAction('copy')}
                sendTo={
                  <SendToPopover
                    onSend={count => notify('Sent', `Arrived on ${count} devices.`)}
                    renderTrigger={Trigger => (
                      <Trigger
                        render={
                          <PushButton size="large" tint="glass" icon="arrowRight">
                            Send to…
                          </PushButton>
                        }
                      />
                    )}
                  />
                }
              />
            </section>
          )}

          <div className="mac-scroll-edge" />

          {!sidebarHidden && (
            <div
              className={`mac-sidebar${generation === '26' ? ' mac-glass' : ''}`}
              role="listbox"
              aria-label="Library"
              tabIndex={0}
              aria-activedescendant={`side-${section}`}
              onKeyDown={onSidebarKeyDown}
            >
              {sidebarSections.map((group, index) => (
                <div
                  key={group.title ?? index}
                  role="group"
                  aria-label={group.title}
                  style={{ display: 'contents' }}
                >
                  {group.title && (
                    <div className="mac-side-heading" aria-hidden="true">
                      {group.title}
                    </div>
                  )}
                  {group.items.map(item => (
                    <SidebarRow
                      key={item.id}
                      id={`side-${item.id}`}
                      item={item}
                      selected={item.id === section}
                      data-force={item.id === demoHoverId ? 'hover' : undefined}
                      onClick={() => setSection(item.id)}
                    />
                  ))}
                </div>
              ))}
            </div>
          )}

          <div className="mac-traffic" aria-hidden="true">
            <span style={{ background: '#FF5F57' }} />
            <span style={{ background: '#FEBC2E' }} />
            <span style={{ background: '#28C840' }} />
          </div>

          <Toolbar.Root className="mac-toolbar" aria-label="Toolbar">
            <span className="mac-glass-group mac-glass">
              <ToolbarButton
                icon="sidebar"
                label="Toggle Sidebar"
                onClick={() => setSidebarHidden(hidden => !hidden)}
              />
            </span>
            <span className="mac-glass-group mac-glass">
              <ToolbarButton icon="chevronLeft" label="Back" disabled />
            </span>
            <span className="mac-toolbar-title">{sectionLabel?.label ?? 'All items'}</span>
            <span className="mac-toolbar-count">{sectionLabel?.count}</span>
            <span className="mac-toolbar-spacer" />
            <SearchField
              width={190}
              placeholder="Search"
              inputRef={searchRef}
              value={query}
              onValueChange={value => setQuery(value)}
            />
            <span className="mac-glass-group mac-glass">
              <ToolbarButton
                icon="copy"
                label="Copy"
                shortcut="⌘C"
                onClick={() => runAction('copy')}
              />
              <SendToPopover
                onSend={count => notify('Sent', `Arrived on ${count} devices.`)}
                renderTrigger={Trigger => (
                  <ToolbarButton
                    icon="arrowRight"
                    label="Send to…"
                    shortcut="⌃⌘S"
                    render={<Trigger />}
                  />
                )}
              />
              <ToolbarButton
                icon="pin"
                label="Pin"
                shortcut="⌘P"
                onClick={() => runAction('pin')}
              />
            </span>
            <span className="mac-glass-group mac-glass">
              <MoreMenuButton onAction={runAction} />
            </span>
          </Toolbar.Root>

          {/* Window-modal surfaces live inside the window, like sheets and alerts attached to an NSWindow. */}
          <PortalContainer value={windowEl}>
            <DeleteAlert
              open={deleteOpen}
              onOpenChange={setDeleteOpen}
              onConfirm={() => {
                setDeleted(list => [...list, ...visibleSelection])
                setSelection([])
              }}
            />
            <TagSheet
              open={tagOpen}
              onOpenChange={setTagOpen}
              onAdd={tag => {
                if (!current) return
                const trimmed = tag.replace(/^#/, '').trim()
                // Ignore an empty name ("#") and a tag the item already has.
                if (trimmed === '' || current.tags.includes(trimmed as TagName)) return
                const name = trimmed as TagName
                setExtraTags(map => ({ ...map, [current.id]: [...(map[current.id] ?? []), name] }))
              }}
            />
            <Toast.Portal container={windowEl}>
              <Toast.Viewport className="mac-toast-viewport">
                <ToastStack />
              </Toast.Viewport>
            </Toast.Portal>
          </PortalContainer>
        </div>
      </Tooltip.Provider>
    </Toast.Provider>
  )
}
