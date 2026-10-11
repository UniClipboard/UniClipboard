import { Menu as FluentMenu, MenuPopover, MenuTrigger } from '@fluentui/react-components'
import type { ContextMenuProps } from '@/ui-kit/contract'
import { useMenuStyles } from './menu-styles'
import { MenuEntries } from './MenuEntries'

/** Fluent `openOnContext`: right-click or Shift+F10 / the context-menu key opens at the target. */
export function ContextMenu({ items, onAction, open, onOpenChange, children }: ContextMenuProps) {
  const styles = useMenuStyles()
  return (
    <FluentMenu openOnContext open={open} onOpenChange={(_, data) => onOpenChange?.(data.open)}>
      <MenuTrigger disableButtonEnhancement>
        <div>{children}</div>
      </MenuTrigger>
      <MenuPopover className={styles.popover}>
        <MenuEntries items={items} onAction={onAction} />
      </MenuPopover>
    </FluentMenu>
  )
}
