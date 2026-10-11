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
// - Window chrome (caption buttons, traffic lights, title-bar dragging), the system menu bar and
//   system notifications belong to the native shell, not to this kit. Window materials are applied
//   by the native host; the kit only marks the regions that show them (`Surface`), and a skin
//   reads the material the host actually applied from the host layer, never from business code.
import type { ReactNode, Ref } from 'react'

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
  | 'info'
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

/**
 * Tag colour as the daemon stores it: a palette name, whose light/dark values the skin owns, or a
 * custom `#rrggbb`. A tag without a colour is drawn neutral.
 */
export type TagPaletteColor = 'orange' | 'blue' | 'green' | 'purple' | 'gray'
export type TagColor = TagPaletteColor | `#${string}`

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

/**
 * Window regions. `sidebar` shows the platform's window material when the native host applied one
 * (macOS Liquid Glass / vibrancy behind a transparent web view); in a browser, or when the host
 * applied none, the skin draws an opaque or CSS fallback instead. `content` is always opaque.
 * Every surface root carries `data-window-material`: `native-liquid-glass`, `native-visual-effect`,
 * `css-fallback` or `solid`, so a browser rendering can never be mistaken for the native material.
 */
export type SurfaceRegion = 'sidebar' | 'content'

/** `region`, not `role`: a surface is a layout region, not an ARIA role on the element. */
export interface SurfaceProps {
  region: SurfaceRegion
  children: ReactNode
}

export interface ButtonProps {
  variant?: ButtonVariant
  size?: ControlSize
  icon?: IconName
  /** Visible text. Omit it for an icon-only button and give `label` instead. */
  children?: ReactNode
  /** Accessible name of an icon-only button; ignored when there is visible text. */
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
  /** Return or Enter in the field: search now instead of waiting for typing to settle. */
  onSubmit?: (value: string) => void
  /** The text input, for the caller's focus shortcut. */
  inputRef?: Ref<HTMLInputElement>
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
  /**
   * `filter`: narrows the list in view (a filter pill in the list's filter row); the placeholder is
   * the filter's name while nothing is chosen.
   */
  purpose?: 'filter'
}

export interface SegmentedItem {
  value: string
  label: string
  icon?: IconName
  /** Show only the icon; `label` stays the accessible name and tooltip. */
  iconOnly?: boolean
}

/**
 * Mutually exclusive options shown side by side, with radio-group semantics: one tab stop, and
 * the arrow keys move focus and selection together.
 */
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
      /** Trailing count, already formatted by the caller, e.g. items in a saved view. */
      count?: string
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
  /** A "do not ask again" checkbox (NSAlert suppression button). */
  suppression?: { label: string; checked: boolean; onCheckedChange: (checked: boolean) => void }
}

export interface BadgeProps {
  count: number
  /** Accessible name, e.g. "3 new items"; a bare number means nothing to a screen reader. */
  label: string
}

export interface TagProps {
  name: string
  color?: TagColor
  /** Shows a remove button named by `remove.label`, e.g. "Remove #finance". */
  remove?: LabelledAction
  /** Secondary text after the name, e.g. "2 of 3". */
  detail?: string
  /** Only some of the selected items carry the tag (drawn striped). */
  partial?: boolean
  /** Makes the tag itself a button, e.g. "Add #docker to all 3". */
  action?: LabelledAction
}

/** A condition shown as a token inside a `TokenSearchField`, e.g. `Tag #deploy`. */
export interface SearchFieldToken {
  id: string
  /** The condition's field, already localized; display only. */
  field: string
  value: string
  /** Colour family hint (design: source green, tag orange, time blue); skins may draw all alike. */
  kind?: 'tag' | 'source' | 'time' | 'type' | 'other'
  remove: LabelledAction
}

export interface SearchSuggestion {
  id: string
  label: string
  /** Heading shown above this item (the first item of a group). */
  group?: string
  /** Trailing text, e.g. "12 items · 4 from arch-desktop". */
  detail?: string
  kind?: SearchFieldToken['kind']
  /** Already applied. */
  selected?: boolean
  /** Matches nothing under the other conditions. */
  muted?: boolean
  /** Creates something new ("Create #re"). */
  create?: boolean
}

/** A key press the caller's search model decides on; `preventDefault` keeps the field from acting. */
export interface SearchFieldKey {
  key: string
  shiftKey: boolean
  preventDefault: () => void
}

/**
 * Search field with condition tokens and a suggestion menu (macOS search tokens, WinUI
 * AutoSuggestBox with tokens). The caller owns the search model: text, tokens, the suggestion list,
 * which one is highlighted and what keys do (`onKeyDown` sees every key first). Each skin implements
 * the ARIA combobox pattern: the input has role `combobox`, `aria-expanded` follows
 * `suggestionsOpen`, `aria-controls` names the suggestion listbox and `aria-activedescendant` the
 * highlighted option.
 */
export interface TokenSearchFieldProps {
  label: string
  placeholder?: string
  /** Display-only shortcut hint such as `⌘F`. */
  shortcutHint?: string
  value: string
  onValueChange: (value: string) => void
  tokens: SearchFieldToken[]
  suggestions: SearchSuggestion[]
  suggestionsOpen: boolean
  onSuggestionsOpenChange: (open: boolean) => void
  /** Highlighted suggestion index, -1 for none; the caller moves it (arrow keys via `onKeyDown`). */
  highlighted: number
  onHighlightedChange: (index: number) => void
  onSelectSuggestion: (index: number) => void
  onKeyDown?: (key: SearchFieldKey) => void
  /** Trailing clear-all button, shown while there is text or a token. */
  clear?: LabelledAction
  /** Display-only footer of the suggestion menu (keyboard hints). */
  suggestionsFooter?: string
  inputRef?: Ref<HTMLInputElement>
}

/** The colour choice offered on a `create` suggestion of a `ComboField`. */
export interface ComboFieldCreateColor {
  label: string
  colors: TagColor[]
  value: TagColor
  onValueChange: (color: TagColor) => void
}

/**
 * A compact text field with a suggestion menu, e.g. the tag field of an item (design MC1). The
 * caller owns the model (text, suggestions, highlighted item, keys); the field reports when the
 * user leaves it (`onDismiss`: focus moved away). Each skin implements the ARIA combobox pattern.
 */
export interface ComboFieldProps {
  label: string
  /** Leading text inside the field, e.g. `#`. */
  prefix?: string
  placeholder?: string
  value: string
  onValueChange: (value: string) => void
  suggestions: SearchSuggestion[]
  highlighted: number
  onHighlightedChange: (index: number) => void
  onSelectSuggestion: (index: number) => void
  onKeyDown?: (key: SearchFieldKey) => void
  onDismiss: () => void
  createColor?: ComboFieldCreateColor
  maxLength?: number
  /** Display-only footer of the menu (keyboard hints). */
  footer?: string
  inputRef?: Ref<HTMLInputElement>
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
  /** Search terms the skin marks in the title (system find highlight). */
  highlight?: string[]
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
  /** Trailing count, already formatted by the caller (`12,408`, `4 of 5`). */
  count?: string
  /** Status dot in place of an icon, e.g. a device's connection. */
  status?: StatusTone
  /** Display-only shortcut hint, e.g. `⌘,` on a settings row. */
  shortcutHint?: string
}

export interface SidebarNavSection {
  id: string
  title?: string
  items: SidebarNavItem[]
  /** A command in the section heading, e.g. "New Tag" (shown as an icon button). */
  action?: LabelledAction & { icon: IconName }
  /** A link row after the items, e.g. "All Tags · 42". */
  more?: LabelledAction
  /** Shown instead of the items while there are none. */
  placeholder?: { text?: string; action?: LabelledAction }
  /**
   * The section as one icon when a skin collapses the sidebar to icons (Windows compact pane), its
   * items then open in a flyout. Skins that hide the sidebar completely ignore it.
   */
  icon?: IconName
}

/** Single-selection navigation (Up/Down move, Enter/Space select, Tab leaves the list). */
export interface SidebarNavProps {
  label: string
  sections: SidebarNavSection[]
  /** Items pinned to the bottom of the sidebar, e.g. Settings; they select like the others. */
  footer?: SidebarNavItem[]
  value?: string
  defaultValue?: string
  onValueChange?: (id: string) => void
}

/**
 * The main window: a sidebar beside the content, with one toolbar band across the top of the
 * content (macOS unified toolbar). The skin owns the sidebar width, the window-chrome insets
 * (macOS traffic lights) and where the show/hide control sits; `sidebar` is drawn on the `sidebar`
 * window region and `children` (the panes) on the opaque `content` region.
 */
export interface SplitViewProps {
  sidebar: ReactNode
  sidebarHidden?: boolean
  /** The control that hides or shows the sidebar; `label` names the action it performs now. */
  sidebarToggle: LabelledAction & { shortcutHint?: string }
  /** Title of the content at the leading end of the toolbar band. */
  title: string
  /** Secondary text after the title, e.g. the item count. */
  subtitle?: string
  /**
   * The window's search field (a `SearchField`). macOS: first item of the toolbar's trailing group;
   * Windows: centred in the title bar.
   */
  search?: ReactNode
  /** Controls at the trailing end of the toolbar band, after the search field. */
  toolbar?: ReactNode
  children: ReactNode
}

/**
 * A column of the content region below the toolbar band. `list` is the browsing column (the
 * history list); `detail` fills the remaining width.
 */
export interface PaneProps {
  kind: 'list' | 'detail'
  label: string
  children: ReactNode
}

/** Text roles; skins map them to the platform type ramp. Business code never sets font sizes. */
export type TextRole = 'title' | 'headline' | 'body' | 'callout' | 'caption'
export type TextTone = 'primary' | 'secondary' | 'tertiary' | 'positive' | 'critical'

export interface TextProps {
  role?: TextRole
  tone?: TextTone
  /** Bold within the role, e.g. a device name inside a sentence. */
  emphasized?: boolean
  monospace?: boolean
  /** One line with an ellipsis. */
  truncate?: boolean
  children: ReactNode
}

/** Clipboard content kinds the kit can draw a badge for. */
export type ContentKind = 'text' | 'code' | 'link' | 'image' | 'file'

/**
 * The type mark of an entry. `tile`: the square leading visual of a list row (an image shows
 * `imageSrc` as a thumbnail). `pill`: the capsule naming the kind in a detail header.
 */
export interface KindBadgeProps {
  kind: ContentKind
  /** Visible text of the mark (`TXT`, `PDF`, `Code`); it is also the accessible name. */
  label: string
  variant: 'tile' | 'pill'
  imageSrc?: string
}

/** A run of consecutive `items` of a `GroupedList` under one heading. */
export interface ListGroup {
  id: string
  title: string
  /** Secondary text after the title, e.g. the date of a day heading. */
  detail?: string
  /** Right-aligned text, already formatted, e.g. "5 items". */
  trailing?: string
  /** How many of the following items belong to this group. */
  count: number
}

/**
 * A long, lazily loaded list of selectable items in groups (a history list). Rows outside the
 * viewport are not rendered. Keyboard: Up/Down move the selection, Shift extends it, Home/End jump,
 * the platform select-all shortcut selects every loaded row, Enter calls `onActivate`, the platform's
 * delete key calls `onDelete` (macOS Delete/⌫ or Forward Delete, Windows Delete). Pointer: a click selects one row, the platform modifier adds or
 * removes a row and Shift selects a range. Rows show multi-selection the platform way (macOS has no
 * checkboxes). Each row element carries `data-item-id`.
 */
export interface GroupedListProps {
  label: string
  groups: ListGroup[]
  items: ItemListItem[]
  /** Ids of the selected rows; the last one is the row the keyboard moves from. */
  selection: string[]
  onSelectionChange: (ids: string[]) => void
  onActivate?: (id: string) => void
  onDelete?: (ids: string[]) => void
  /** Called near the end of the loaded rows, to load more. */
  onEndReached?: () => void
  /** Context menu of the selection, or of the right-clicked row when it is not selected. */
  contextMenu?: {
    items: (ids: string[]) => MenuEntry[]
    onAction: (id: string, ids: string[]) => void
  }
  /** Content after the last row, e.g. a loading indicator or a hint. */
  footer?: ReactNode
}

/**
 * A symbol inline with content. Without `label` it is decorative (hidden from assistive tech).
 * `highlight` is the attention colour of a marked state, e.g. a pinned item (macOS yellow).
 */
export interface IconProps {
  name: IconName
  label?: string
  tone?: 'secondary' | 'highlight'
}

/** A floating bar of the main actions at the bottom of a pane. */
export interface ActionBarProps {
  children: ReactNode
  /** Display-only hint at the trailing end, e.g. keyboard shortcuts. */
  hint?: string
}

/** Empty, no-results and placeholder states of a pane (AppKit "content unavailable"). */
export interface ContentUnavailableProps {
  icon?: IconName
  title: string
  description?: string
  /** Actions or choices below the description. */
  children?: ReactNode
}

export interface StepListStep {
  id: string
  title: string
  description?: string
  /** Live status under the description, e.g. "Waiting for your first copy…"; `active` shows it in progress. */
  status?: { text: string; active?: boolean }
  /** The step's command, e.g. "Pair a Device…". */
  action?: LabelledAction & { primary?: boolean }
  done?: boolean
}

/** Numbered getting-started steps (macOS cards with numbered badges, WinUI setup-guide list). */
export interface StepListProps {
  label: string
  steps: StepListStep[]
}

export type StatusTone = 'positive' | 'neutral' | 'warning' | 'critical'

export interface InsetListRow {
  id: string
  title: string
  /** Trailing status text, coloured by `tone`. */
  detail?: string
  tone?: StatusTone
  action?: LabelledAction
}

/** A grouped (inset) table of short rows with a status dot, e.g. per-device delivery. */
export interface InsetListProps {
  label: string
  rows: InsetListRow[]
}

export declare function UiRoot(props: UiRootProps): ReactNode
export declare function Surface(props: SurfaceProps): ReactNode
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
export declare function TokenSearchField(props: TokenSearchFieldProps): ReactNode
export declare function ComboField(props: ComboFieldProps): ReactNode
export declare function InlineNotice(props: InlineNoticeProps): ReactNode
export declare function Toolbar(props: ToolbarProps): ReactNode
export declare function ToolbarButton(props: ToolbarButtonProps): ReactNode
export declare function ToolbarDivider(): ReactNode
export declare function ItemList(props: ItemListProps): ReactNode
export declare function SidebarNav(props: SidebarNavProps): ReactNode
export declare function SplitView(props: SplitViewProps): ReactNode
export declare function Pane(props: PaneProps): ReactNode
export declare function Text(props: TextProps): ReactNode
export declare function KindBadge(props: KindBadgeProps): ReactNode
export declare function GroupedList(props: GroupedListProps): ReactNode
export declare function Icon(props: IconProps): ReactNode
export declare function ActionBar(props: ActionBarProps): ReactNode
export declare function ContentUnavailable(props: ContentUnavailableProps): ReactNode
export declare function StepList(props: StepListProps): ReactNode
export declare function InsetList(props: InsetListProps): ReactNode
