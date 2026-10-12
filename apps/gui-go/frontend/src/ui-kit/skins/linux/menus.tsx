import { AlertDialog as BaseAlertDialog } from '@base-ui/react/alert-dialog'
import { ContextMenu as BaseContextMenu } from '@base-ui/react/context-menu'
import { Menu as BaseMenu } from '@base-ui/react/menu'
import { useId, useRef, useState } from 'react'
import type { AlertDialogProps, ContextMenuProps, MenuEntry, MenuProps } from '@/ui-kit/contract'
import { WithHelpTag } from './controls'
import { glyph } from './glyph'
import { Icon } from './icons'
import { Checkbox, PushButton } from './parts'
import { usePortalContainer } from './portal'
import { Spinner } from './spinner'
import { swatchColor } from './swatch'

/**
 * Open state of a menu. Choosing an item closes the menu first and then sends the action.
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

/** The items of a menu panel; an item with `children` opens a submenu (trailing chevron). */
function MenuItems({ items, onSelect }: { items: MenuEntry[]; onSelect: (id: string) => void }) {
  const container = usePortalContainer()
  return items.map(entry => {
    if ('kind' in entry) return <BaseMenu.Separator key={entry.id} className="adw-menu-separator" />
    const content = (
      <>
        <span className="adw-menu-icon">
          {entry.icon && <Icon name={glyph(entry.icon)} size={16} />}
          {entry.swatch && (
            <span
              className="adw-dot-tag"
              data-checked={entry.checked ? '' : undefined}
              style={{ background: swatchColor(entry.swatch) }}
            />
          )}
          {entry.checked && !entry.icon && !entry.swatch && (
            <Icon name="check" size={14} strokeWidth={2.2} />
          )}
        </span>
        <span className="adw-menu-label">{entry.label}</span>
        {entry.count !== undefined && <span className="adw-menu-shortcut">{entry.count}</span>}
        {entry.shortcut && <span className="adw-menu-shortcut">{entry.shortcut}</span>}
      </>
    )
    if (entry.children) {
      return (
        <BaseMenu.SubmenuRoot key={entry.id}>
          <BaseMenu.SubmenuTrigger
            className="adw-menu-item"
            disabled={entry.disabled || entry.children.length === 0}
          >
            {content}
            <span className="adw-menu-shortcut">
              <Icon name="chevronRight" size={14} strokeWidth={2.2} />
            </span>
          </BaseMenu.SubmenuTrigger>
          <BaseMenu.Portal container={container}>
            <BaseMenu.Positioner className="adw-positioner" sideOffset={2} align="start">
              <BaseMenu.Popup className="adw-menu">
                <MenuItems items={entry.children} onSelect={onSelect} />
              </BaseMenu.Popup>
            </BaseMenu.Positioner>
          </BaseMenu.Portal>
        </BaseMenu.SubmenuRoot>
      )
    }
    return (
      <BaseMenu.Item
        key={entry.id}
        className="adw-menu-item"
        data-destructive={entry.destructive ? '' : undefined}
        disabled={entry.disabled}
        closeOnClick={false}
        onClick={() => onSelect(entry.id)}
      >
        {content}
      </BaseMenu.Item>
    )
  })
}

/** Popover menu panel shared by action menus and context menus. */
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
      <BaseMenu.Positioner className="adw-positioner" sideOffset={4} align="start">
        <BaseMenu.Popup className="adw-menu">
          <MenuItems items={items} onSelect={onSelect} />
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
            <span className="adw-badge adw-badge-inline">{count}</span>
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

/** Right-click inside `children` opens the menu at the pointer. */
export function ContextMenu({ items, onAction, open, onOpenChange, children }: ContextMenuProps) {
  const { isOpen, change } = useMenuState(open, onOpenChange)
  return (
    <BaseContextMenu.Root open={isOpen} onOpenChange={next => change(next)}>
      <BaseContextMenu.Trigger render={<div className="adw-context-area" />}>
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
 * AdwAlertDialog: centred heading and body, then Cancel and the confirming action side by side as
 * pill buttons. A destructive alert keeps keyboard focus on Cancel, so Enter never confirms data
 * loss by accident.
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
  suppression,
  field,
  confirmDisabled,
  busyLabel,
  error,
}: AlertDialogProps) {
  const container = usePortalContainer()
  // Focus lands on the alert's key action: Cancel when the action is destructive, the field when
  // the alert asks for typed confirmation.
  const confirmRef = useRef<HTMLButtonElement>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const fieldRef = useRef<HTMLInputElement>(null)
  const fieldId = useId()
  const busy = busyLabel !== undefined
  return (
    <BaseAlertDialog.Root
      open={open}
      onOpenChange={(next, details) => {
        // Running work cannot be dismissed (Escape, a click outside).
        if (!next && busy) {
          details.cancel()
          return
        }
        onOpenChange(next)
      }}
    >
      <BaseAlertDialog.Portal container={container}>
        <BaseAlertDialog.Backdrop className="adw-alert-backdrop" data-fixed="" />
        <BaseAlertDialog.Popup
          className="adw-alert"
          data-floating=""
          data-fixed=""
          initialFocus={field ? fieldRef : destructive ? cancelRef : confirmRef}
        >
          <BaseAlertDialog.Title className="adw-alert-title">{title}</BaseAlertDialog.Title>
          <BaseAlertDialog.Description className="adw-alert-text">
            {description}
          </BaseAlertDialog.Description>
          {suppression && (
            <label className="adw-check-label adw-alert-suppression">
              <Checkbox
                checked={suppression.checked}
                onCheckedChange={checked => suppression.onCheckedChange(checked === true)}
              />
              {suppression.label}
            </label>
          )}
          {field && (
            <div className="adw-alert-field">
              <label htmlFor={fieldId}>{field.label}</label>
              <input
                id={fieldId}
                ref={fieldRef}
                className="adw-form-input"
                value={field.value}
                placeholder={field.placeholder}
                autoComplete="off"
                spellCheck={false}
                disabled={busy}
                onChange={event => field.onValueChange(event.target.value)}
              />
            </div>
          )}
          {error && (
            <p className="adw-alert-error" role="alert">
              {error}
            </p>
          )}
          <div className="adw-alert-actions">
            <BaseAlertDialog.Close
              ref={cancelRef}
              disabled={busy}
              render={<PushButton size="large" />}
            >
              {cancelLabel}
            </BaseAlertDialog.Close>
            <PushButton
              ref={confirmRef}
              size="large"
              tint={destructive ? 'destructive' : 'primary'}
              disabled={confirmDisabled || busy}
              aria-busy={busy || undefined}
              onClick={() => {
                const result = onConfirm()
                if (!(result instanceof Promise)) onOpenChange(false)
              }}
            >
              {busy && <Spinner size={14} />}
              {busy ? busyLabel : confirmLabel}
            </PushButton>
          </div>
        </BaseAlertDialog.Popup>
      </BaseAlertDialog.Portal>
    </BaseAlertDialog.Root>
  )
}
