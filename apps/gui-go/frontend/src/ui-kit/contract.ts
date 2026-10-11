// Public contract of the platform UI kit.
//
// Business code imports components only from `@/ui-kit`. The implementation behind it is chosen
// when the bundle is built: `UC_UI_PLATFORM=windows|macos` makes vite resolve `@ui-skin` to
// `src/ui-kit/skins/<platform>/index.ts` (see `vite.config.ts`), so one bundle contains exactly
// one skin and never decides the platform at runtime. For the type checker `@ui-skin` resolves to
// this file, and every skin proves with its `conformance.ts` that it exports exactly these names
// with these signatures.
//
// Rules for the contract:
// - Props describe meaning, not a vendor API. Callbacks carry only the new value
//   (`onValueChange(value)`, `onCheckedChange(checked)`, `onOpenChange(open)`, `onAction(id)`);
//   skins translate Fluent `(event, data)` or Base UI `(value, eventDetails)` internally.
// - Controlled and uncontrolled forms come in pairs (`value`/`defaultValue`).
// - Sizes are semantic steps and icons are `IconName`s; no pixels, class names or vendor props.
// - Window chrome (caption buttons, traffic lights, title-bar dragging), window materials (Mica,
//   Liquid Glass), the system menu bar and system notifications belong to the native shell, not to
//   this kit.
import type { ReactNode } from 'react'

export type ColorScheme = 'light' | 'dark'

/** Icons every skin maps to its platform icon set (Fluent System Icons, SF Symbols style). */
export type IconName =
  | 'add'
  | 'back'
  | 'chevronDown'
  | 'chevronRight'
  | 'copy'
  | 'delete'
  | 'desktop'
  | 'devices'
  | 'dismiss'
  | 'edit'
  | 'filter'
  | 'history'
  | 'moon'
  | 'more'
  | 'pin'
  | 'search'
  | 'send'
  | 'settings'
  | 'sidebar'
  | 'sun'
  | 'tag'

/**
 * `xlarge` is the prominent primary action of an action bar (macOS 40pt capsule); skins without a
 * larger step render it as `large`.
 */
export type ControlSize = 'small' | 'medium' | 'large' | 'xlarge'

/**
 * `primary`: the single main action of a view (Windows accent button, macOS default button).
 * `standard`: an ordinary push button. `subtle`: command-bar and icon buttons without a fill at rest.
 */
export type ButtonVariant = 'primary' | 'standard' | 'subtle'

/** Project tag palette; skins own the light/dark values. */
export type TagColor = 'deploy' | 'docker' | 'git' | 'design' | 'finance' | 'work' | 'neutral'

/**
 * A secondary command of a component. `label` is the visible text or, for an icon-only control,
 * the accessible name; it comes from the caller's translations, skins never supply copy.
 */
export interface LabelledAction {
  label: string
  onClick: () => void
}

export interface UiRootProps {
  /** Comes from the app's existing theme source; the kit keeps no theme state of its own. */
  colorScheme: ColorScheme
  children: ReactNode
}

export interface ButtonProps {
  variant?: ButtonVariant
  size?: ControlSize
  icon?: IconName
  /** Visible text. Omit it for an icon-only button and give `label` instead. */
  children?: ReactNode
  /** Accessible name; required when there is no visible text. */
  label?: string
  /** Hover/focus hint. For an icon-only button it defaults to `label`. */
  tooltip?: string
  disabled?: boolean
  onClick?: () => void
}

export interface ToggleButtonProps {
  pressed?: boolean
  defaultPressed?: boolean
  onPressedChange?: (pressed: boolean) => void
  size?: ControlSize
  icon?: IconName
  children?: ReactNode
  label?: string
  tooltip?: string
  disabled?: boolean
}

export interface CheckedControlProps {
  checked?: boolean
  defaultChecked?: boolean
  onCheckedChange?: (checked: boolean) => void
  /** Accessible name; shown next to the control unless `labelHidden`. */
  label: string
  labelHidden?: boolean
  disabled?: boolean
}

export type SwitchProps = CheckedControlProps
export type CheckboxProps = CheckedControlProps

export interface SearchFieldProps {
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  placeholder?: string
  label: string
  /** Display-only shortcut hint such as `Ctrl+F`; registering the shortcut is the caller's job. */
  shortcutHint?: string
}

export interface SelectItem {
  value: string
  label: string
}

/** Single choice from a short list (Windows drop-down, macOS pop-up button). */
export interface SelectProps {
  items: SelectItem[]
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  label: string
  placeholder?: string
  disabled?: boolean
}

export interface SegmentedItem {
  value: string
  label: string
  icon?: IconName
  /** Show only the icon; `label` stays the accessible name and tooltip. */
  iconOnly?: boolean
}

/** Mutually exclusive options shown side by side (radio-group semantics, arrow-key navigation). */
export interface SegmentedControlProps {
  items: SegmentedItem[]
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  label: string
}

export type MenuEntry =
  | {
      id: string
      label: string
      icon?: IconName
      /** Display-only shortcut hint. */
      shortcut?: string
      destructive?: boolean
      disabled?: boolean
      /** Trailing count, e.g. items in a saved view. */
      count?: number
      swatch?: TagColor
    }
  | { id: string; kind: 'separator' }

interface MenuItemsProps {
  items: MenuEntry[]
  onAction: (id: string) => void
  open?: boolean
  onOpenChange?: (open: boolean) => void
}

/** A button that opens an action menu. The menu owns its trigger so skins control focus return. */
export interface MenuProps extends MenuItemsProps {
  label: string
  icon?: IconName
  iconOnly?: boolean
  variant?: ButtonVariant
  /** Count badge on the trigger, e.g. active filter values. */
  count?: number
}

/** Opens `items` at the pointer on right-click or the context-menu key inside `children`. */
export interface ContextMenuProps extends MenuItemsProps {
  children: ReactNode
}

export interface AlertDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  confirmLabel: string
  cancelLabel: string
  /** The confirmed action destroys data; each skin applies its platform's destructive styling. */
  destructive?: boolean
  onConfirm: () => void
}

export interface BadgeProps {
  count: number
  label?: string
}

export interface TagProps {
  name: string
  color?: TagColor
  /** Shows a remove button named by `remove.label`, e.g. "Remove #finance". */
  remove?: LabelledAction
}

/** A search condition chip such as `from: iPhone 16`. */
export interface SearchTokenProps {
  field: string
  value: string
  remove?: LabelledAction
}

export type NoticeTone = 'info' | 'success' | 'warning' | 'error'

/** Inline status message pinned in the content, not a floating toast. */
export interface InlineNoticeProps {
  tone: NoticeTone
  title: string
  text?: string
  action?: LabelledAction
  /** Shows a close button named by `dismiss.label`. */
  dismiss?: LabelledAction
}

/** A row of commands with roving focus (arrow keys move between items, Tab leaves the bar). */
export interface ToolbarProps {
  label: string
  children: ReactNode
}

export type ToolbarButtonProps = Omit<ButtonProps, 'size'>

export interface ItemListItem {
  id: string
  title: string
  subtitle?: string
  monospace?: boolean
  /** Leading visual such as a content-type badge or thumbnail. */
  leading?: ReactNode
  /** Trailing content such as tags or a pin mark. */
  trailing?: ReactNode
}

/**
 * Selectable item list with listbox semantics. Keyboard behaviour is the same on every platform:
 * Up/Down move focus, Home/End jump, Space toggles selection, Enter or double click calls
 * `onActivate`, Delete calls `onDelete` with the selection. With `multiSelect` each row shows a
 * checkbox; how a pointer click extends the selection follows the platform (Windows ListView
 * "Multiple" mode: click toggles; macOS: Cmd+click adds).
 */
export interface ItemListProps {
  label: string
  items: ItemListItem[]
  selection?: string[]
  defaultSelection?: string[]
  onSelectionChange?: (ids: string[]) => void
  multiSelect?: boolean
  onActivate?: (id: string) => void
  onDelete?: (ids: string[]) => void
}

export interface SidebarNavItem {
  id: string
  label: string
  icon?: IconName
  /** Colour dot or square for smart views and tags. */
  swatch?: TagColor
  count?: string
}

export interface SidebarNavSection {
  id: string
  title?: string
  items: SidebarNavItem[]
}

/** Single-selection navigation (Up/Down move, Enter/Space select, Tab leaves the list). */
export interface SidebarNavProps {
  label: string
  sections: SidebarNavSection[]
  value?: string
  defaultValue?: string
  onValueChange?: (id: string) => void
}

export declare function UiRoot(props: UiRootProps): ReactNode
export declare function Button(props: ButtonProps): ReactNode
export declare function ToggleButton(props: ToggleButtonProps): ReactNode
export declare function Switch(props: SwitchProps): ReactNode
export declare function Checkbox(props: CheckboxProps): ReactNode
export declare function SearchField(props: SearchFieldProps): ReactNode
export declare function Select(props: SelectProps): ReactNode
export declare function SegmentedControl(props: SegmentedControlProps): ReactNode
export declare function Menu(props: MenuProps): ReactNode
export declare function ContextMenu(props: ContextMenuProps): ReactNode
export declare function AlertDialog(props: AlertDialogProps): ReactNode
export declare function Badge(props: BadgeProps): ReactNode
export declare function Tag(props: TagProps): ReactNode
export declare function SearchToken(props: SearchTokenProps): ReactNode
export declare function InlineNotice(props: InlineNoticeProps): ReactNode
export declare function Toolbar(props: ToolbarProps): ReactNode
export declare function ToolbarButton(props: ToolbarButtonProps): ReactNode
export declare function ToolbarDivider(): ReactNode
export declare function ItemList(props: ItemListProps): ReactNode
export declare function SidebarNav(props: SidebarNavProps): ReactNode
