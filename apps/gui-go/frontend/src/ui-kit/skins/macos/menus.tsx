import { AlertDialog as BaseAlertDialog } from '@base-ui/react/alert-dialog'
import { ContextMenu as BaseContextMenu } from '@base-ui/react/context-menu'
import { Menu as BaseMenu } from '@base-ui/react/menu'
import { useRef, useState } from 'react'
import type { AlertDialogProps, ContextMenuProps, MenuEntry, MenuProps } from '@/ui-kit/contract'
import { WithHelpTag } from './controls'
import { glyph } from './glyph'
import { Icon } from './icons'
import { PushButton, usePortalContainer } from './parts'
import { swatchColor } from './swatch'

/**
 * Open state of a menu. Choosing an item closes the menu first and then sends the action, the order
 * AppKit uses (the action is sent after menu tracking ends).
 */
function useMenuState(
  open: boolean | undefined,
  onOpenChange: ((open: boolean) => void) | undefined
) {
  const [own, setOwn] = useState(false)
  const change = (next: boolean) => {
    if (open === undefined) setOwn(next)
    onOpenChange?.(next)
  }
  return { isOpen: open ?? own, change }
}

/** Menu panel shared by action menus and context menus: Tahoe puts a symbol column on every item. */
export function MenuPanel({
  items,
  onSelect,
}: {
  items: MenuEntry[]
  onSelect: (id: string) => void
}) {
  const container = usePortalContainer()
  return (
    <BaseMenu.Portal container={container}>
      <BaseMenu.Positioner className="mac-positioner" sideOffset={4} align="start">
        <BaseMenu.Popup className="mac-menu">
          {items.map(entry =>
            'kind' in entry ? (
              <BaseMenu.Separator key={entry.id} className="mac-menu-separator" />
            ) : (
              <BaseMenu.Item
                key={entry.id}
                className="mac-menu-item"
                data-destructive={entry.destructive ? '' : undefined}
                disabled={entry.disabled}
                closeOnClick={false}
                onClick={() => onSelect(entry.id)}
              >
                <span className="mac-menu-icon">
                  {entry.icon && <Icon name={glyph(entry.icon)} size={14} />}
                  {entry.swatch && (
                    <span
                      className="mac-dot-tag"
                      style={{ background: swatchColor[entry.swatch] }}
                    />
                  )}
                </span>
                <span className="mac-menu-label">{entry.label}</span>
                {entry.count !== undefined && (
                  <span className="mac-menu-shortcut">{entry.count}</span>
                )}
                {entry.shortcut && <span className="mac-menu-shortcut">{entry.shortcut}</span>}
              </BaseMenu.Item>
            )
          )}
        </BaseMenu.Popup>
      </BaseMenu.Positioner>
    </BaseMenu.Portal>
  )
}

export function Menu({
  label,
  icon,
  iconOnly,
  variant = 'standard',
  count,
  items,
  onAction,
  open,
  onOpenChange,
}: MenuProps) {
  const { isOpen, change } = useMenuState(open, onOpenChange)
  return (
    <BaseMenu.Root open={isOpen} onOpenChange={next => change(next)}>
      <WithHelpTag text={iconOnly ? label : undefined}>
        <BaseMenu.Trigger
          render={
            <PushButton
              tint={
                variant === 'primary' ? 'primary' : variant === 'subtle' ? 'borderless' : 'none'
              }
              icon={icon && glyph(icon)}
              aria-label={iconOnly ? label : undefined}
              data-icon-only={iconOnly ? '' : undefined}
            />
          }
        >
          {!iconOnly && label}
          {count !== undefined && count > 0 && (
            <span className="mac-badge mac-badge-inline">{count}</span>
          )}
        </BaseMenu.Trigger>
      </WithHelpTag>
      <MenuPanel
        items={items}
        onSelect={id => {
          change(false)
          onAction(id)
        }}
      />
    </BaseMenu.Root>
  )
}

/** Right-click or Control-click inside `children` opens the menu at the pointer. */
export function ContextMenu({ items, onAction, open, onOpenChange, children }: ContextMenuProps) {
  const { isOpen, change } = useMenuState(open, onOpenChange)
  return (
    <BaseContextMenu.Root open={isOpen} onOpenChange={next => change(next)}>
      <BaseContextMenu.Trigger render={<div className="mac-context-area" />}>
        {children}
      </BaseContextMenu.Trigger>
      <MenuPanel
        items={items}
        onSelect={id => {
          change(false)
          onAction(id)
        }}
      />
    </BaseContextMenu.Root>
  )
}

/**
 * Tahoe alert: centred, capsule buttons stacked vertically. A destructive alert shows the trash
 * symbol and keeps keyboard focus on Cancel, so Return never confirms data loss by accident.
 */
export function AlertDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  cancelLabel,
  destructive,
  onConfirm,
}: AlertDialogProps) {
  const container = usePortalContainer()
  // Focus lands on the alert's key action: Cancel when the action is destructive.
  const confirmRef = useRef<HTMLButtonElement>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  return (
    <BaseAlertDialog.Root open={open} onOpenChange={next => onOpenChange(next)}>
      <BaseAlertDialog.Portal container={container}>
        <BaseAlertDialog.Backdrop className="mac-alert-backdrop" data-fixed="" />
        <BaseAlertDialog.Popup
          className="mac-alert"
          data-floating=""
          data-fixed=""
          initialFocus={destructive ? cancelRef : confirmRef}
        >
          {destructive && (
            <span className="mac-alert-icon">
              <Icon name="trash" size={26} strokeWidth={1.6} color="var(--label-2)" />
            </span>
          )}
          <BaseAlertDialog.Title className="mac-alert-title">{title}</BaseAlertDialog.Title>
          <BaseAlertDialog.Description className="mac-alert-text">
            {description}
          </BaseAlertDialog.Description>
          <div className="mac-alert-actions">
            <PushButton
              ref={confirmRef}
              size="large"
              tint={destructive ? 'destructive' : 'primary'}
              onClick={() => {
                onConfirm()
                onOpenChange(false)
              }}
            >
              {confirmLabel}
            </PushButton>
            <BaseAlertDialog.Close ref={cancelRef} render={<PushButton size="large" />}>
              {cancelLabel}
            </BaseAlertDialog.Close>
          </div>
        </BaseAlertDialog.Popup>
      </BaseAlertDialog.Portal>
    </BaseAlertDialog.Root>
  )
}
