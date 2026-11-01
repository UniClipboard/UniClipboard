import { Dialog } from '@base-ui/react/dialog'
import { Menu } from '@base-ui/react/menu'
import { useState, type ReactNode } from 'react'
import type { BoardId } from '@/prototypes/macos-design/board-ids'
import {
  ActionBar,
  ClipRow,
  CodeCard,
  DeliveryList,
  InlineNotice,
  NotificationCard,
  SidebarRow,
} from '@/prototypes/macos-design/content'
import { clips, sidebarSections } from '@/prototypes/macos-design/data'
import {
  StaticAlert,
  StaticMenu,
  StaticPopover,
  StaticTooltip,
} from '@/prototypes/macos-design/overlays'
import { ClipboardWindow } from '@/prototypes/macos-design/window'
import { Icon } from '@/ui-kit/skins/macos/icons'
import {
  Badge,
  Checkbox,
  PopUpButton,
  PushButton,
  SearchField,
  Segmented,
  Switch,
  Tag,
} from '@/ui-kit/skins/macos/parts'
import { PortalContainer, usePortalContainer } from '@/ui-kit/skins/macos/portal'
import { macTheme } from '@/ui-kit/skins/macos/theme'

type Theme = 'light' | 'dark'

function Panel({ theme, height, children }: { theme: Theme; height: number; children: ReactNode }) {
  const [element, setElement] = useState<HTMLElement | null>(null)
  return (
    <section
      ref={setElement}
      className="board-panel mac-theme"
      data-mac-theme={theme}
      style={{ ...macTheme(theme, null), left: theme === 'light' ? 40 : 870, height }}
      aria-label={theme === 'light' ? 'Light' : 'Dark'}
    >
      <span className="board-panel-label">{theme.toUpperCase()}</span>
      <PortalContainer value={element}>
        <div className="board-panel-body">{children}</div>
      </PortalContainer>
    </section>
  )
}

function Board({
  title,
  subtitle,
  height,
  render,
}: {
  title: string
  subtitle?: string
  height: number
  render: (theme: Theme) => ReactNode
}) {
  return (
    <main className="board" style={{ height }}>
      <header className="board-heading">
        <h1>{title}</h1>
        {subtitle && <p>{subtitle}</p>}
      </header>
      <Panel theme="light" height={height - 130}>
        {render('light')}
      </Panel>
      <Panel theme="dark" height={height - 130}>
        {render('dark')}
      </Panel>
    </main>
  )
}

function Section({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="board-section">
      <div className="board-section-title">
        <h2>{title}</h2>
        <span>{note}</span>
      </div>
      {children}
    </section>
  )
}

function Specimen({ caption, children }: { caption: string; children: ReactNode }) {
  return (
    <div className="board-specimen">
      {children}
      <span className="board-caption">{caption}</span>
    </div>
  )
}

/* ---------- C1 buttons & inputs ---------- */

function ControlsPanel() {
  return (
    <>
      <Section title="Push button · 尺寸" note="Tahoe：小号是圆角矩形，大号是胶囊">
        <div className="board-specimens">
          <Specimen caption="small 20">
            <PushButton size="small">Edit</PushButton>
          </Specimen>
          <Specimen caption="medium 24">
            <PushButton icon="arrowRight">Send to…</PushButton>
          </Specimen>
          <Specimen caption="large 32 · 胶囊">
            <PushButton size="large" icon="arrowRight">
              Send to…
            </PushButton>
          </Specimen>
          <Specimen caption="extra large 40 · 胶囊">
            <PushButton size="xlarge" tint="primary" icon="copy">
              Copy
            </PushButton>
          </Specimen>
        </div>
      </Section>
      <Section title="Push button · 着色强度">
        <div className="board-specimens">
          <Specimen caption="none">
            <PushButton>Cancel</PushButton>
          </Specimen>
          <Specimen caption="secondary">
            <PushButton tint="secondary" icon="pin">
              Pin
            </PushButton>
          </Specimen>
          <Specimen caption="primary · 默认按钮 ↵">
            <PushButton tint="primary">Copy</PushButton>
          </Specimen>
          <Specimen caption="destructive（低强度红）">
            <PushButton tint="destructive" icon="trash">
              Delete
            </PushButton>
          </Specimen>
          <Specimen caption="pressed">
            <PushButton tint="primary" force="pressed">
              Copy
            </PushButton>
          </Specimen>
          <Specimen caption="focus">
            <PushButton tint="primary" force="focus">
              Copy
            </PushButton>
          </Specimen>
          <Specimen caption="disabled">
            <PushButton tint="primary" disabled>
              Copy
            </PushButton>
          </Specimen>
        </div>
      </Section>
      <Section title="Glass 按钮（叠在内容上）" note="工具栏、浮动操作用这个，不用于表单">
        <div className="board-wall">
          <PushButton size="large" tint="glass" icon="copy">
            Copy
          </PushButton>
          <span className="mac-glass-group mac-glass">
            <button type="button" className="mac-glass-item" aria-label="Copy">
              <Icon name="copy" size={16} strokeWidth={1.7} />
            </button>
            <button type="button" className="mac-glass-item" aria-label="Send to">
              <Icon name="arrowRight" size={16} strokeWidth={1.7} />
            </button>
            <button type="button" className="mac-glass-item" aria-label="Pin" data-pressed="">
              <Icon name="pin" size={16} strokeWidth={1.7} />
            </button>
          </span>
          <PushButton size="large" tint="glass" icon="more" aria-label="More" />
        </div>
      </Section>
      <Section title="搜索框 · 工具栏胶囊">
        <div className="board-specimens">
          <Specimen caption="rest">
            <SearchField width={250} placeholder="Search history" />
          </Specimen>
          <Specimen caption="focus">
            <SearchField
              width={250}
              placeholder="Search history"
              defaultValue="docker"
              force="focus"
            />
          </Specimen>
          <Specimen caption="token">
            <SearchField
              width={300}
              placeholder="Search history"
              tokens={['#deploy', 'from: iPhone']}
            />
          </Specimen>
        </div>
      </Section>
      <Section title="其它">
        <div className="board-specimens">
          <Specimen caption="Segmented">
            <Segmented
              label="View"
              defaultValue="list"
              items={[
                { value: 'list', label: 'List', icon: 'list' },
                { value: 'grid', label: 'Grid', icon: 'grid' },
              ]}
            />
          </Specimen>
          <Specimen caption="Pop-up">
            <PopUpButton
              label="Sort order"
              defaultValue="newest"
              items={[
                { value: 'newest', label: 'Newest first' },
                { value: 'oldest', label: 'Oldest first' },
                { value: 'device', label: 'By device' },
              ]}
            />
          </Specimen>
          <Specimen caption="Switch off">
            <Switch aria-label="Sync images" />
          </Specimen>
          <Specimen caption="on">
            <Switch aria-label="Sync text" defaultChecked />
          </Specimen>
          <Specimen caption="Checkbox">
            <Checkbox aria-label="Include pinned" />
          </Specimen>
          <Specimen caption="on">
            <Checkbox aria-label="Include deleted" defaultChecked variant="plain" />
          </Specimen>
          <Specimen caption="Badge">
            <Badge>4</Badge>
          </Specimen>
        </div>
      </Section>
    </>
  )
}

/* ---------- C2 window, toolbar, sidebar ---------- */

const allItems = sidebarSections[0].items[0]

function WindowPanel() {
  return (
    <>
      <Section
        title="窗口 · macOS 26 Tahoe"
        note="侧栏是悬浮玻璃（内缩 8pt，同心圆角）；工具栏按钮按组放在玻璃胶囊里"
      >
        <ClipboardWindow
          generation="26"
          width="100%"
          height={330}
          showDetail={false}
          initialScrollTop={18}
          demoHoverId="tag-deploy"
          alwaysActive
        />
      </Section>
      <Section
        title="窗口 · macOS 27 Golden Gate"
        note="侧栏贴到窗口边缘；工具栏统一；玻璃对比度更高、阴影更柔"
      >
        <ClipboardWindow
          generation="27"
          width="100%"
          height={330}
          showDetail={false}
          initialScrollTop={18}
          demoHoverId="tag-deploy"
          alwaysActive
        />
      </Section>
      <Section title="侧栏项 · 状态" note="选中：浅灰圆角底 + 强调色图标（不是整行蓝底）">
        <div className="board-specimens" role="listbox" aria-label="Sidebar states">
          <Specimen caption="rest">
            <div style={{ width: 200 }}>
              <SidebarRow item={allItems} />
            </div>
          </Specimen>
          <Specimen caption="hover">
            <div style={{ width: 200 }}>
              <SidebarRow item={allItems} data-force="hover" />
            </div>
          </Specimen>
          <Specimen caption="sel">
            <div style={{ width: 200 }}>
              <SidebarRow item={allItems} selected />
            </div>
          </Specimen>
        </div>
      </Section>
    </>
  )
}

/* ---------- C3 list, tags, tokens ---------- */

const clip = (id: string) => clips.find(item => item.id === id)!

function RowSpecimen({ caption, children }: { caption: string; children: ReactNode }) {
  return (
    <div className="board-row-specimen">
      <div style={{ width: 560 }}>{children}</div>
      <span className="board-caption">{caption}</span>
    </div>
  )
}

function ListPanel() {
  return (
    <>
      <Section title="分组标题" note="Headline + Subheadline">
        <div className="mac-section-header" style={{ width: 560 }}>
          <span style={{ fontWeight: 700 }}>Today</span>
          <span style={{ fontSize: 11, color: 'var(--label-2)' }}>Mon, Sep 28</span>
          <span style={{ flexGrow: 1 }} />
          <span style={{ fontSize: 11, color: 'var(--label-3)' }}>5 items</span>
        </div>
      </Section>
      <Section title="列表行" note="选中 = 圆角强调色底 + 白字；窗口失焦变灰">
        <div
          role="listbox"
          aria-label="Row states"
          style={{ display: 'flex', flexDirection: 'column', gap: 3 }}
        >
          <RowSpecimen caption="rest">
            <ClipRow clip={clip('rebase')} />
          </RowSpecimen>
          <RowSpecimen caption="selected · 窗口在前台">
            <ClipRow clip={clip('compose-up')} selected />
          </RowSpecimen>
          <RowSpecimen caption="selected · 窗口在后台">
            <div data-window-inactive="">
              <ClipRow clip={clip('compose-up')} selected />
            </div>
          </RowSpecimen>
          <RowSpecimen caption="image">
            <ClipRow clip={clip('screenshot')} />
          </RowSpecimen>
          <RowSpecimen caption="multi-select">
            <ClipRow clip={clip('roadmap')} selected multiSelect />
          </RowSpecimen>
          <RowSpecimen caption="multi-select">
            <ClipRow clip={clip('invoice')} multiSelect={false} />
          </RowSpecimen>
        </div>
      </Section>
      <Section title="Tag / 搜索 token" note="全部胶囊形">
        <div className="board-specimens">
          <Specimen caption="tag">
            <Tag label="#deploy" color="orange" />
          </Specimen>
          <Specimen caption="tag">
            <Tag label="#docker" color="blue" />
          </Specimen>
          <Specimen caption="可删除">
            <Tag
              label="#finance"
              color="green"
              remove={{ label: 'Remove #finance', onClick: () => {} }}
            />
          </Specimen>
          <Specimen caption="token">
            <span className="mac-token">from: iPhone 16</span>
          </Specimen>
          <Specimen caption="add">
            <button type="button" className="mac-tag-add">
              + Add tag
            </button>
          </Specimen>
        </div>
      </Section>
    </>
  )
}

/* ---------- C4 detail parts ---------- */

function DetailPanel() {
  const code = clip('compose-up').body!
  return (
    <>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 20 }}>
        <Section title="代码卡片">
          <CodeCard body={code} lines={3} chars={74} />
        </Section>
        <Section title="分组列表 · Delivery" note="System Settings 式">
          <DeliveryList />
        </Section>
      </div>
      <Section title="内联提示（macOS 没有 InfoBar，用浅色圆角块）">
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <InlineNotice
            tone="blue"
            title="Menu bar icon hidden"
            text="Turn it on in Control Center settings."
            action={<PushButton size="small">Open Settings</PushButton>}
          />
          <InlineNotice tone="green" title="Sent" text="Arrived on 2 devices." />
          <InlineNotice tone="orange" title="Relay only" text="Direct connection blocked." />
          <InlineNotice
            tone="red"
            title="Not delivered"
            text="Pixel 8 rejected images."
            action={<PushButton size="small">Retry</PushButton>}
          />
        </div>
      </Section>
      <Section title="底部操作条" note="主操作用 XL 胶囊；整条可以是浮在内容上的玻璃">
        <div
          style={{
            padding: 14,
            borderRadius: 16,
            background: 'var(--content)',
            boxShadow: 'inset 0 0 0 0.5px var(--separator)',
          }}
        >
          <ActionBar />
        </div>
      </Section>
    </>
  )
}

/* ---------- C5 overlays ---------- */

function OverlayPanel() {
  return (
    <>
      <div className="board-wall" style={{ borderRadius: 16, gap: 20, alignItems: 'flex-start' }}>
        <Section title="右键菜单">
          <StaticMenu />
        </Section>
        <Section title="Popover">
          <StaticPopover />
        </Section>
      </div>
      <div style={{ display: 'flex', gap: 24, alignItems: 'flex-start' }}>
        <Section title="Alert（Tahoe 起居中、胶囊按钮竖排）">
          <StaticAlert />
        </Section>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 22 }}>
          <Section title="帮助提示">
            <StaticTooltip>Pin (⌘P)</StaticTooltip>
          </Section>
          <Section title="通知（系统绘制，这里只定内容）">
            <NotificationCard />
          </Section>
        </div>
      </div>
    </>
  )
}

/* ---------- T3 materials and motion (interactive lab, not a pixel replica) ---------- */

function MorphSearch() {
  const [expanded, setExpanded] = useState(false)
  // A label: clicking anywhere on the capsule focuses the field natively, which expands it.
  return (
    <label className="lab-morph mac-glass" data-expanded={expanded ? '' : undefined}>
      <Icon name="search" size={16} />
      <input
        aria-label="Search"
        placeholder="Search"
        tabIndex={expanded ? 0 : -1}
        onFocus={() => setExpanded(true)}
        onBlur={event => {
          if (event.currentTarget.value === '') setExpanded(false)
        }}
      />
    </label>
  )
}

function SheetDemo() {
  const [element, setElement] = useState<HTMLDivElement | null>(null)
  return (
    <div ref={setElement} className="lab-mini-window">
      <div style={{ padding: 12, display: 'flex', justifyContent: 'center', marginTop: 40 }}>
        <Dialog.Root>
          <Dialog.Trigger render={<PushButton>Show sheet</PushButton>} />
          <Dialog.Portal container={element}>
            <Dialog.Backdrop className="mac-sheet-backdrop" />
            <Dialog.Popup
              className="mac-sheet"
              style={{ width: 170, marginLeft: -85, padding: 12 }}
            >
              <Dialog.Title className="mac-sheet-title">Sheet</Dialog.Title>
              <Dialog.Close render={<PushButton tint="primary">Done</PushButton>} />
            </Dialog.Popup>
          </Dialog.Portal>
        </Dialog.Root>
      </div>
    </div>
  )
}

function LabPanel({ theme }: { theme: Theme }) {
  const container = usePortalContainer()
  return (
    <>
      <Section
        title="Glass 变体"
        note="Regular / Clear / Tinted / Prominent（backdrop-filter 近似，无折射）"
      >
        <div className="board-specimens">
          {(['regular', 'clear', 'tinted', 'prominent'] as const).map(variant => (
            <Specimen key={variant} caption={variant}>
              <div className="lab-tile">
                <div className="lab-glass-sample mac-glass" data-variant={variant}>
                  <Icon name="copy" size={17} />
                  <Icon name="arrowRight" size={17} />
                  <Icon name="pin" size={17} />
                </div>
              </div>
            </Specimen>
          ))}
        </div>
      </Section>
      <Section title="动效（可交互）" note="弹簧缩放 · 变形 · 菜单从按钮长出 · Sheet 从标题栏落下">
        <div className="board-specimens">
          <Specimen caption="按住：弹簧缩放">
            <div className="lab-tile">
              <span
                className="mac-glass-group mac-glass"
                style={{ height: 44, borderRadius: 22, padding: '0 6px' }}
              >
                <button type="button" className="mac-glass-item" aria-label={`Pin (${theme})`}>
                  <Icon name="pin" size={16} strokeWidth={1.7} />
                </button>
              </span>
            </div>
          </Specimen>
          <Specimen caption="点击：变形为搜索框">
            <div className="lab-tile">
              <MorphSearch />
            </div>
          </Specimen>
          <Specimen caption="菜单从按钮长出">
            <div className="lab-tile">
              <Menu.Root>
                <Menu.Trigger
                  render={<PushButton size="large" tint="glass" icon="more" aria-label="Actions" />}
                />
                <Menu.Portal container={container}>
                  <Menu.Positioner className="mac-positioner" sideOffset={6}>
                    <Menu.Popup className="mac-menu" style={{ minWidth: 160 }}>
                      {(['Copy', 'Send to…', 'Pin'] as const).map(label => (
                        <Menu.Item key={label} className="mac-menu-item">
                          <span className="mac-menu-label">{label}</span>
                        </Menu.Item>
                      ))}
                    </Menu.Popup>
                  </Menu.Positioner>
                </Menu.Portal>
              </Menu.Root>
            </div>
          </Specimen>
          <Specimen caption="Sheet 从标题栏落下">
            <SheetDemo />
          </Specimen>
        </div>
      </Section>
    </>
  )
}

const boards: Record<BoardId, () => ReactNode> = {
  c1: () => (
    <Board
      title="macOS 组件 · 1 按钮与输入"
      subtitle="同一套组件，只切换主题变量"
      height={820}
      render={() => <ControlsPanel />}
    />
  ),
  c2: () => (
    <Board
      title="macOS 组件 · 2 窗口、工具栏、侧栏"
      subtitle="26 与 27 的差别都在这一张"
      height={1100}
      render={() => <WindowPanel />}
    />
  ),
  c3: () => (
    <Board
      title="macOS 组件 · 3 列表、标签、搜索 token"
      height={720}
      render={() => <ListPanel />}
    />
  ),
  c4: () => <Board title="macOS 组件 · 4 详情区" height={760} render={() => <DetailPanel />} />,
  c5: () => (
    <Board
      title="macOS 组件 · 5 浮层"
      subtitle="菜单 / Popover / 通知是玻璃；Alert 是更不透明的玻璃"
      height={820}
      render={() => <OverlayPanel />}
    />
  ),
  t3: () => (
    <Board
      title="macOS · 材质与动效（交互演示）"
      subtitle="对应设计 T3；用于手动体验，不做像素比对"
      height={760}
      render={theme => <LabPanel theme={theme} />}
    />
  ),
}

/** One specimen board at its design-canvas size. */
export function BoardPage({ id }: { id: BoardId }) {
  const Board = boards[id]
  return <Board />
}
