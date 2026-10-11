// Component gallery of the platform UI kit. This file is platform-neutral business code: it
// imports only `@/ui-kit`, and the skin comes from `UC_UI_PLATFORM` at build time. The layout
// follows the component boards (C1 to C5) of the desktop design canvas.
//   UC_UI_PLATFORM=windows UI_FIXTURE_ENTRY=e2e/fixtures/ui-kit-gallery.tsx node e2e/visual-effects-server.mjs
// `?scheme=light|dark` renders one scheme; the default shows both side by side like the boards.
import { useState } from 'react'
import type { ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import {
  AlertDialog,
  Badge,
  Button,
  Checkbox,
  ContextMenu,
  InlineNotice,
  ItemList,
  Menu,
  SearchField,
  SearchToken,
  SegmentedControl,
  Select,
  SidebarNav,
  Surface,
  Switch,
  Tag,
  ToggleButton,
  Toolbar,
  ToolbarButton,
  ToolbarDivider,
  UiRoot,
} from '@/ui-kit'
import type { ColorScheme, ItemListItem, MenuEntry, SidebarNavSection } from '@/ui-kit'
import '@/App.css'

const clipActions: MenuEntry[] = [
  { id: 'copy', label: 'Copy', icon: 'copy', shortcut: 'Enter' },
  { id: 'send', label: 'Send to…', icon: 'send', shortcut: 'Ctrl+Shift+S' },
  { id: 'edit', label: 'Edit', icon: 'edit', shortcut: 'E' },
  { id: 'tag', label: 'Add tag…', icon: 'tag', shortcut: 'T' },
  { id: 'pin', label: 'Pin', icon: 'pin', shortcut: 'Ctrl+P' },
  { id: 'sep', kind: 'separator' },
  {
    id: 'delete',
    label: 'Delete from all devices',
    icon: 'delete',
    shortcut: 'Del',
    destructive: true,
  },
]

const smartViews: MenuEntry[] = [
  { id: 'deploy', label: 'Deploy commands', swatch: 'deploy', count: '14' },
  { id: 'iphone', label: 'From iPhone today', swatch: 'docker', count: '6' },
  { id: 'invoices', label: 'Invoices', swatch: 'finance', count: '9' },
  { id: 'sep', kind: 'separator' },
  { id: 'save', label: 'Save current search…', icon: 'add' },
]

const navSections: SidebarNavSection[] = [
  {
    id: 'library',
    items: [
      { id: 'all', label: 'All items', icon: 'copy', count: '12,408' },
      { id: 'pinned', label: 'Pinned', icon: 'pin', count: '24' },
      { id: 'deleted', label: 'Recently deleted', icon: 'delete', count: '12' },
    ],
  },
  {
    id: 'views',
    title: 'Smart views',
    items: [
      { id: 'deploy', label: 'Deploy commands', swatch: 'deploy', count: '14' },
      { id: 'invoices', label: 'Invoices', swatch: 'finance', count: '9' },
    ],
  },
  {
    id: 'tags',
    title: 'Tags',
    items: [
      { id: 'docker', label: '#docker', swatch: 'docker', count: '38' },
      { id: 'tag-deploy', label: '#deploy', swatch: 'deploy', count: '14' },
    ],
  },
]

function Kind({ children }: { children: string }) {
  return <span className="ui-kit-gallery-kind">{children}</span>
}

const clips: ItemListItem[] = [
  {
    id: 'rebase',
    title: 'git rebase -i HEAD~3',
    subtitle: 'arch-desktop · Alacritty · 09:38:12',
    monospace: true,
    leading: <Kind>{'</>'}</Kind>,
    trailing: <Tag name="#git" color="git" />,
  },
  {
    id: 'url',
    title: 'https://github.com/UniClipboard/UniClipboard',
    subtitle: 'iPhone 16 · Safari · 09:37:40',
    leading: <Kind>URL</Kind>,
  },
  {
    id: 'compose',
    title: 'docker compose -f compose.prod.yml up -d',
    subtitle: 'arch-desktop · Alacritty · 22:14:08',
    monospace: true,
    leading: <Kind>{'</>'}</Kind>,
    trailing: (
      <>
        <Tag name="#deploy" color="deploy" />
        <Tag name="#docker" color="docker" />
      </>
    ),
  },
  {
    id: 'roadmap',
    title: 'Q4 roadmap: ship LAN-only Mode, then per-device gates',
    subtitle: 'this PC · Obsidian · 09:26:51',
    leading: <Kind>TXT</Kind>,
    trailing: <Tag name="#work" color="work" />,
  },
  {
    id: 'invoice',
    title: 'invoice-0921.pdf',
    subtitle: 'Pixel 8 · Files · 08:40:19',
    leading: <Kind>PDF</Kind>,
    trailing: <Tag name="#finance" color="finance" />,
  },
]

function Section({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="ui-kit-gallery-section" aria-label={title}>
      <h2>
        {title}
        {note ? <small>{note}</small> : null}
      </h2>
      <div className="ui-kit-gallery-row">{children}</div>
    </section>
  )
}

function Board({ scheme }: { scheme: ColorScheme }) {
  const [log, setLog] = useState('ready')
  const [pinned, setPinned] = useState(false)
  const [query, setQuery] = useState('')
  const [dialogOpen, setDialogOpen] = useState(false)
  const [selection, setSelection] = useState(['compose'])
  const [nav, setNav] = useState('all')
  const record = (event: string) => setLog(event)
  return (
    <UiRoot colorScheme={scheme}>
      <div className="ui-kit-gallery-board" data-testid={`board-${scheme}`}>
        <p className="ui-kit-gallery-scheme">{scheme.toUpperCase()}</p>
        <output data-testid="event-log">{log}</output>

        <Section title="Buttons" note="C1 · accent, standard, subtle, toggle">
          <Button variant="primary" icon="copy" onClick={() => record('click:copy')}>
            Copy
          </Button>
          <Button icon="send" onClick={() => record('click:send')}>
            Send to…
          </Button>
          <Button variant="subtle" icon="edit" onClick={() => record('click:edit')}>
            Edit
          </Button>
          <Button variant="subtle" icon="more" label="More" onClick={() => record('click:more')} />
          <Button variant="primary" icon="copy" disabled>
            Copy
          </Button>
          <ToggleButton
            icon="pin"
            pressed={pinned}
            onPressedChange={next => {
              setPinned(next)
              record(`pressed:${next}`)
            }}
          >
            {pinned ? 'Pinned' : 'Pin'}
          </ToggleButton>
        </Section>

        <Section title="Search" note="C1 · AutoSuggestBox">
          <div className="ui-kit-gallery-search">
            <SearchField
              label="Search history"
              placeholder="Search, or type tag: from:"
              shortcutHint="Ctrl+F"
              value={query}
              onValueChange={next => {
                setQuery(next)
                record(`query:${next}`)
              }}
            />
          </div>
        </Section>

        <Section
          title="Filters and choices"
          note="C1 · DropDownButton, segmented, switch, checkbox"
        >
          <Menu label="Type" items={smartViews} onAction={id => record(`filter:${id}`)} />
          <Menu label="Tag" count={1} items={smartViews} onAction={id => record(`filter:${id}`)} />
          <Select
            label="Device"
            items={[
              { value: 'all', label: 'All devices' },
              { value: 'iphone', label: 'iPhone 16' },
              { value: 'pixel', label: 'Pixel 8' },
            ]}
            defaultValue="all"
            onValueChange={next => record(`device:${next}`)}
          />
          <SegmentedControl
            label="Theme"
            defaultValue="light"
            items={[
              { value: 'light', label: 'Light', icon: 'sun', iconOnly: true },
              { value: 'dark', label: 'Dark', icon: 'moon', iconOnly: true },
              { value: 'system', label: 'System', icon: 'desktop', iconOnly: true },
            ]}
            onValueChange={next => record(`theme:${next}`)}
          />
          <Switch label="Sync" onCheckedChange={next => record(`sync:${next}`)} />
          <Checkbox
            label="Select"
            defaultChecked
            onCheckedChange={next => record(`check:${next}`)}
          />
        </Section>

        <div className="ui-kit-gallery-split">
          <Section title="Navigation" note="C2 · NavigationView pane">
            <Surface region="sidebar">
              <SidebarNav
                label="Library"
                sections={navSections}
                value={nav}
                onValueChange={next => {
                  setNav(next)
                  record(`nav:${next}`)
                }}
              />
            </Surface>
          </Section>

          <Section title="List" note="C3 · ListViewItem, tags, query chips">
            <Surface region="content">
              <div className="ui-kit-gallery-list">
                <ContextMenu
                  items={clipActions}
                  onAction={id => {
                    record(`context:${id}`)
                    if (id === 'delete') setDialogOpen(true)
                  }}
                >
                  <ItemList
                    label="Today"
                    items={clips}
                    selection={selection}
                    onSelectionChange={ids => {
                      setSelection(ids)
                      record(`selection:${ids.join(',')}`)
                    }}
                    onActivate={id => record(`activate:${id}`)}
                    onDelete={ids => record(`delete:${ids.join(',')}`)}
                  />
                </ContextMenu>
                <div className="ui-kit-gallery-row">
                  <Tag name="#deploy" color="deploy" />
                  <Tag
                    name="#finance"
                    color="finance"
                    remove={{ label: 'Remove #finance', onClick: () => record('remove:#finance') }}
                  />
                  <SearchToken
                    field="from:"
                    value="iPhone 16"
                    remove={{
                      label: 'Remove from: iPhone 16',
                      onClick: () => record('remove:from'),
                    }}
                  />
                  <SearchToken field="time:" value="last 7 days" />
                  <Badge count={3} label="3 new items" />
                </div>
                <ItemList
                  label="Multi-select"
                  multiSelect
                  items={clips.slice(3)}
                  defaultSelection={['roadmap']}
                  onSelectionChange={ids => record(`multi:${ids.join(',')}`)}
                />
              </div>
            </Surface>
          </Section>
        </div>

        <Section title="Command bar" note="C4 · primary accent, the rest subtle">
          <div className="ui-kit-gallery-wide">
            <Toolbar label="Item commands">
              <ToolbarButton variant="primary" icon="copy" onClick={() => record('cmd:copy')}>
                Copy
              </ToolbarButton>
              <ToolbarButton icon="send" onClick={() => record('cmd:send')}>
                Send to…
              </ToolbarButton>
              <ToolbarButton icon="edit" onClick={() => record('cmd:edit')}>
                Edit
              </ToolbarButton>
              <ToolbarDivider />
              <ToolbarButton icon="delete" onClick={() => setDialogOpen(true)}>
                Delete
              </ToolbarButton>
              <Menu
                label="More commands"
                icon="more"
                iconOnly
                variant="subtle"
                items={clipActions}
                onAction={id => record(`menu:${id}`)}
                onOpenChange={open => record(`menu-open:${open}`)}
              />
            </Toolbar>
          </div>
        </Section>

        <Section title="Notices" note="C4 · InfoBar">
          <div className="ui-kit-gallery-wide ui-kit-gallery-stack">
            <InlineNotice
              tone="info"
              title="Keep the cat visible"
              text="Windows hides new tray icons under ^."
              action={{ label: 'Open settings', onClick: () => record('notice:settings') }}
              dismiss={{ label: 'Dismiss', onClick: () => record('notice:dismiss') }}
            />
            <InlineNotice tone="success" title="Sent" text="Arrived on 2 devices." />
            <InlineNotice
              tone="warning"
              title="Relay only"
              text="Direct connection blocked by the network."
            />
            <InlineNotice
              tone="error"
              title="Not delivered"
              text="Pixel 8 rejected images."
              action={{ label: 'Retry', onClick: () => record('notice:retry') }}
            />
          </div>
        </Section>

        <AlertDialog
          open={dialogOpen}
          onOpenChange={setDialogOpen}
          title="Delete from all devices?"
          description="This item is removed from 4 devices. You can restore it from Recently deleted for 30 days."
          confirmLabel="Delete"
          cancelLabel="Cancel"
          destructive
          onConfirm={() => record('dialog:confirm')}
        />
      </div>
    </UiRoot>
  )
}

const galleryCss = `
body { margin: 0; background: #EDEEF0; }
.ui-kit-gallery { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 40rem), 1fr)); gap: 1.25rem; padding: 1.5rem; }
.ui-kit-gallery-board { padding: 1.25rem 1.5rem 2rem; border-radius: 0.5rem; display: flex; flex-direction: column; gap: 1.25rem; }
.ui-kit-gallery-scheme { margin: 0; font-size: 0.75rem; letter-spacing: 0.04em; opacity: 0.6; }
.ui-kit-gallery-section h2 { margin: 0 0 0.625rem; font-size: 0.875rem; font-weight: 600; display: flex; gap: 0.625rem; align-items: baseline; }
.ui-kit-gallery-section h2 small { font-weight: 400; font-size: 0.75rem; opacity: 0.6; }
.ui-kit-gallery-row { display: flex; flex-wrap: wrap; gap: 0.5rem; align-items: center; }
.ui-kit-gallery-search { width: min(100%, 22rem); }
.ui-kit-gallery-split > section > .ui-kit-gallery-row > * { flex: 1 1 auto; }
.ui-kit-gallery-split { display: grid; grid-template-columns: minmax(14rem, 18rem) minmax(0, 1fr); gap: 1.25rem; }
.ui-kit-gallery-list { display: flex; flex-direction: column; gap: 0.75rem; padding: 0.5rem; }
.ui-kit-gallery-wide { width: 100%; }
.ui-kit-gallery-stack { display: flex; flex-direction: column; gap: 0.5rem; }
.ui-kit-gallery-kind { display: inline-grid; place-items: center; width: 2rem; height: 2rem; flex-shrink: 0; border-radius: 0.25rem; font-size: 0.625rem; font-weight: 600; background: rgba(127,127,127,0.16); }
output[data-testid="event-log"] { font-family: monospace; font-size: 0.75rem; opacity: 0.6; }
@media (max-width: 40rem) { .ui-kit-gallery-split { grid-template-columns: minmax(0, 1fr); } }
`

export default function Gallery() {
  const param = new URLSearchParams(window.location.search).get('scheme')
  const schemes: ColorScheme[] = param === 'light' || param === 'dark' ? [param] : ['light', 'dark']
  return (
    <>
      <style>{galleryCss}</style>
      <main className="ui-kit-gallery" data-testid="ui-kit-gallery">
        {schemes.map(scheme => (
          <Board key={scheme} scheme={scheme} />
        ))}
      </main>
    </>
  )
}

createRoot(document.getElementById('root')!).render(<Gallery />)
