import { AlertDialog } from '@base-ui/react/alert-dialog'
import { ContextMenu } from '@base-ui/react/context-menu'
import { Dialog } from '@base-ui/react/dialog'
import { Menu } from '@base-ui/react/menu'
import { Popover } from '@base-ui/react/popover'
import { Toast } from '@base-ui/react/toast'
import { Toolbar } from '@base-ui/react/toolbar'
import { Tooltip } from '@base-ui/react/tooltip'
import { useState, type ReactElement, type ReactNode } from 'react'
import { sendTargets } from '@/prototypes/macos-design/data'
import { Icon, type IconName } from '@/ui-kit/skins/macos/icons'
import { Checkbox, PushButton } from '@/ui-kit/skins/macos/parts'
import { usePortalContainer } from '@/ui-kit/skins/macos/portal'

export type ClipAction = 'copy' | 'send' | 'edit' | 'tag' | 'pin' | 'delete'

const clipActions: { id: ClipAction; label: string; icon: IconName; shortcut: string }[] = [
  { id: 'copy', label: 'Copy', icon: 'copy', shortcut: '↩' },
  { id: 'send', label: 'Send to…', icon: 'arrowRight', shortcut: '⌃⌘S' },
  { id: 'edit', label: 'Edit', icon: 'pencil', shortcut: 'E' },
  { id: 'tag', label: 'Add Tag…', icon: 'tag', shortcut: 'T' },
  { id: 'pin', label: 'Pin', icon: 'pin', shortcut: '⌘P' },
]

const deleteAction = {
  id: 'delete' as const,
  label: 'Delete from All Devices',
  icon: 'trash' as const,
  shortcut: '⌫',
}

function MenuRow({ label, icon, shortcut }: { label: string; icon: IconName; shortcut: string }) {
  return (
    <>
      <span className="mac-menu-icon">
        <Icon name={icon} size={14} />
      </span>
      <span className="mac-menu-label">{label}</span>
      <span className="mac-menu-shortcut">{shortcut}</span>
    </>
  )
}

/** The item menu, shared by the right-click menu and the toolbar "more" menu. */
function ClipMenuPopup({ onAction }: { onAction: (action: ClipAction) => void }) {
  const container = usePortalContainer()
  return (
    <Menu.Portal container={container}>
      <Menu.Positioner className="mac-positioner" sideOffset={4} align="start">
        <Menu.Popup className="mac-menu">
          {clipActions.map(action => (
            <Menu.Item
              key={action.id}
              className="mac-menu-item"
              onClick={() => onAction(action.id)}
            >
              <MenuRow {...action} />
            </Menu.Item>
          ))}
          <Menu.Separator className="mac-menu-separator" />
          <Menu.Item
            className="mac-menu-item"
            data-destructive=""
            onClick={() => onAction('delete')}
          >
            <MenuRow {...deleteAction} />
          </Menu.Item>
        </Menu.Popup>
      </Menu.Positioner>
    </Menu.Portal>
  )
}

export function ClipContextMenu({
  children,
  onAction,
}: {
  children: ReactElement
  onAction: (action: ClipAction) => void
}) {
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger render={children} />
      <ClipMenuPopup onAction={onAction} />
    </ContextMenu.Root>
  )
}

/** Toolbar capsule button with a help tag; arrow keys move between buttons of one Toolbar. */
export function ToolbarButton({
  icon,
  label,
  shortcut,
  render,
  ...props
}: {
  icon: IconName
  label: string
  shortcut?: string
  render?: ReactElement
  onClick?: () => void
  disabled?: boolean
}) {
  const container = usePortalContainer()
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        render={
          <Toolbar.Button
            render={render}
            className="mac-glass-item"
            aria-label={label}
            {...props}
          />
        }
      >
        <Icon name={icon} size={16} strokeWidth={1.7} />
      </Tooltip.Trigger>
      <Tooltip.Portal container={container}>
        <Tooltip.Positioner className="mac-positioner" sideOffset={8}>
          <Tooltip.Popup className="mac-tooltip">
            {label}
            {shortcut && ` (${shortcut})`}
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

export function MoreMenuButton({ onAction }: { onAction: (action: ClipAction) => void }) {
  return (
    <Menu.Root>
      <ToolbarButton icon="more" label="More" render={<Menu.Trigger />} />
      <ClipMenuPopup onAction={onAction} />
    </Menu.Root>
  )
}

function SendToBody({
  onSend,
  renderCheckbox,
}: {
  onSend?: (count: number) => void
  renderCheckbox: boolean
}) {
  const [checked, setChecked] = useState(() =>
    sendTargets.filter(t => t.defaultChecked).map(t => t.id)
  )
  const toggle = (id: string, on: boolean) =>
    setChecked(current => (on ? [...current, id] : current.filter(item => item !== id)))
  return (
    <>
      {sendTargets.map(target => (
        <label key={target.id} className="mac-popover-row">
          <Checkbox
            checked={checked.includes(target.id)}
            onCheckedChange={on => toggle(target.id, on)}
            tabIndex={renderCheckbox ? undefined : -1}
          />
          {target.name}
          <span className="mac-popover-row-meta">{target.route}</span>
        </label>
      ))}
      <div className="mac-popover-footer">
        <PushButton
          tint="primary"
          disabled={checked.length === 0}
          onClick={() => onSend?.(checked.length)}
        >
          Send to {checked.length}
        </PushButton>
      </div>
    </>
  )
}

export function SendToPopover({
  renderTrigger,
  onSend,
}: {
  renderTrigger: (Trigger: typeof Popover.Trigger) => ReactNode
  onSend: (count: number) => void
}) {
  const container = usePortalContainer()
  const [open, setOpen] = useState(false)
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      {renderTrigger(Popover.Trigger)}
      <Popover.Portal container={container}>
        <Popover.Positioner className="mac-positioner" sideOffset={8}>
          <Popover.Popup className="mac-popover">
            <Popover.Title className="mac-popover-title">Send to</Popover.Title>
            <SendToBody
              renderCheckbox
              onSend={count => {
                setOpen(false)
                onSend(count)
              }}
            />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}

const alertCopy = {
  title: 'Delete from all devices?',
  text: 'Removed from 4 devices. Restore from Recently Deleted within 30 days.',
}

function AlertBody({
  title,
  text,
  actions,
}: {
  title: ReactNode
  text: ReactNode
  actions: ReactNode
}) {
  return (
    <>
      <span className="mac-alert-icon">
        <Icon name="trash" size={26} strokeWidth={1.6} color="var(--label-2)" />
      </span>
      {title}
      {text}
      <div className="mac-alert-actions">{actions}</div>
    </>
  )
}

export function DeleteAlert({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void
}) {
  const container = usePortalContainer()
  return (
    <AlertDialog.Root open={open} onOpenChange={onOpenChange}>
      <AlertDialog.Portal container={container}>
        <AlertDialog.Backdrop className="mac-alert-backdrop" />
        <AlertDialog.Popup className="mac-alert" data-floating="">
          <AlertBody
            title={
              <AlertDialog.Title className="mac-alert-title">{alertCopy.title}</AlertDialog.Title>
            }
            text={
              <AlertDialog.Description className="mac-alert-text">
                {alertCopy.text}
              </AlertDialog.Description>
            }
            actions={
              <>
                <PushButton
                  size="large"
                  tint="destructive"
                  onClick={() => {
                    onConfirm()
                    onOpenChange(false)
                  }}
                >
                  Delete
                </PushButton>
                <AlertDialog.Close render={<PushButton size="large">Cancel</PushButton>} />
              </>
            }
          />
        </AlertDialog.Popup>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  )
}

export function TagSheet({
  open,
  onOpenChange,
  onAdd,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onAdd: (tag: string) => void
}) {
  const container = usePortalContainer()
  const [value, setValue] = useState('')
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal container={container}>
        <Dialog.Backdrop className="mac-sheet-backdrop" />
        <Dialog.Popup className="mac-sheet">
          <Dialog.Title className="mac-sheet-title">Add Tag</Dialog.Title>
          <form
            style={{ display: 'contents' }}
            onSubmit={event => {
              event.preventDefault()
              onAdd(value)
              setValue('')
              onOpenChange(false)
            }}
          >
            <input
              className="mac-field"
              aria-label="Tag name"
              placeholder="#tag"
              value={value}
              onChange={event => setValue(event.target.value)}
            />
            <div className="mac-sheet-actions">
              <Dialog.Close render={<PushButton>Cancel</PushButton>} />
              <PushButton tint="primary" type="submit" disabled={value.trim() === ''}>
                Add
              </PushButton>
            </div>
          </form>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

export function ToastStack() {
  const { toasts } = Toast.useToastManager()
  return toasts.map(toast => (
    <Toast.Root key={toast.id} toast={toast} className="mac-toast">
      <Toast.Content className="mac-notification mac-glass">
        <span className="mac-notification-icon" />
        <div className="mac-notification-body">
          <Toast.Title className="mac-notification-title" />
          <Toast.Description className="mac-notification-text" />
        </div>
      </Toast.Content>
    </Toast.Root>
  ))
}

/* ---------- static renders for the specimen boards (same classes, no open-state machinery) ---------- */

export function StaticMenu() {
  return (
    <div className="mac-menu" style={{ width: 250 }} role="presentation">
      {clipActions.map((action, index) => (
        <div
          key={action.id}
          className="mac-menu-item"
          data-force={index === 0 ? 'highlighted' : undefined}
        >
          <MenuRow {...action} />
        </div>
      ))}
      <div className="mac-menu-separator" />
      <div className="mac-menu-item" data-destructive="">
        <MenuRow {...deleteAction} />
      </div>
    </div>
  )
}

export function StaticPopover() {
  return (
    <div className="mac-popover" role="presentation">
      <span className="mac-popover-title">Send to</span>
      <SendToBody renderCheckbox={false} />
    </div>
  )
}

export function StaticAlert() {
  return (
    <div className="mac-alert" role="presentation">
      <AlertBody
        title={<span className="mac-alert-title">{alertCopy.title}</span>}
        text={<span className="mac-alert-text">{alertCopy.text}</span>}
        actions={
          <>
            <PushButton size="large" tint="destructive">
              Delete
            </PushButton>
            <PushButton size="large">Cancel</PushButton>
          </>
        }
      />
    </div>
  )
}

export function StaticTooltip({ children }: { children: ReactNode }) {
  return (
    <span className="mac-tooltip" style={{ alignSelf: 'flex-start' }}>
      {children}
    </span>
  )
}
