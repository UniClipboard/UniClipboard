import { Button } from '@base-ui/react/button'
import { Checkbox as BaseCheckbox } from '@base-ui/react/checkbox'
import { Input } from '@base-ui/react/input'
import { Radio } from '@base-ui/react/radio'
import { RadioGroup } from '@base-ui/react/radio-group'
import { Select } from '@base-ui/react/select'
import { Switch as BaseSwitch } from '@base-ui/react/switch'
import { useState, type ComponentProps, type ReactNode } from 'react'
import type { TagColor } from '@/ui-kit/contract'
import { Icon, type IconName } from './icons'
import { usePortalContainer } from './portal'
import { isCustomColor } from './swatch'

/** Static boards force a visual state on a real control; interactive surfaces never pass it. */
export type ForcedState = 'pressed' | 'focus' | 'hover' | 'highlighted'

export function PushButton({
  size = 'medium',
  tint = 'none',
  icon,
  force,
  children,
  ...props
}: {
  size?: 'small' | 'medium' | 'large' | 'xlarge'
  tint?: 'none' | 'primary' | 'secondary' | 'destructive' | 'borderless' | 'link'
  icon?: IconName
  force?: ForcedState
} & ComponentProps<typeof Button>) {
  // libadwaita symbolic icons are 16 px at every button size.
  const iconSize = size === 'small' ? 14 : 16
  return (
    <Button
      className="adw-btn"
      data-size={size}
      data-tint={tint}
      data-force={force}
      focusableWhenDisabled={false}
      {...props}
    >
      {icon && <Icon name={icon} size={iconSize} />}
      {children}
    </Button>
  )
}

export function SearchField({
  width = '100%',
  placeholder,
  label = placeholder,
  hint,
  tokens,
  force,
  inputRef,
  defaultValue,
  ...props
}: {
  width?: number | string
  placeholder?: string
  label?: string
  hint?: string
  tokens?: string[]
  force?: ForcedState
  inputRef?: React.Ref<HTMLInputElement>
} & Omit<ComponentProps<typeof Input>, 'ref'>) {
  return (
    <label className="adw-search" style={{ width }} data-force={force}>
      <Icon name="search" size={16} />
      {tokens?.map(token => (
        <span key={token} className="adw-token">
          {token}
        </span>
      ))}
      <Input
        ref={inputRef}
        type="search"
        aria-label={label}
        placeholder={tokens?.length ? undefined : placeholder}
        defaultValue={defaultValue}
        {...props}
      />
      {hint && <kbd>{hint}</kbd>}
    </label>
  )
}

/**
 * Linked button group in select-one mode: a toolbar holding a radio group, so arrow keys move and
 * select and exactly one segment stays selected.
 */
export function Segmented({
  items,
  value,
  defaultValue,
  onValueChange,
  label,
}: {
  items: { value: string; label: string; icon?: IconName; iconOnly?: boolean }[]
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  label: string
}) {
  const [own, setOwn] = useState(defaultValue)
  const current = value ?? own
  return (
    <div role="toolbar" aria-label={label} className="adw-segmented">
      <RadioGroup
        aria-label={label}
        className="adw-segmented-group"
        value={current ?? null}
        onValueChange={next => {
          if (typeof next !== 'string') return
          if (value === undefined) setOwn(next)
          onValueChange?.(next)
        }}
      >
        {items.map(item => (
          <Radio.Root
            key={item.value}
            value={item.value}
            className="adw-segment"
            aria-label={item.iconOnly ? item.label : undefined}
            title={item.iconOnly ? item.label : undefined}
          >
            {item.icon && <Icon name={item.icon} size={16} />}
            {!item.iconOnly && item.label}
          </Radio.Root>
        ))}
      </RadioGroup>
    </div>
  )
}

export function PopUpButton({
  items,
  value,
  defaultValue,
  onValueChange,
  label,
  placeholder,
  disabled,
  purpose,
}: {
  items: { value: string; label: string }[]
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  label: string
  placeholder?: string
  disabled?: boolean
  purpose?: 'filter'
}) {
  const container = usePortalContainer()
  // A controlled value that is not one of the items is "nothing chosen": the placeholder shows.
  const controlled =
    value === undefined ? undefined : items.some(item => item.value === value) ? value : null
  return (
    <Select.Root
      items={items}
      value={controlled}
      defaultValue={defaultValue}
      onValueChange={next => {
        if (next !== null) onValueChange?.(next)
      }}
      disabled={disabled}
      modal={false}
    >
      <Select.Trigger
        className={purpose === 'filter' ? 'adw-filter-pill' : 'adw-popup-btn'}
        data-active={purpose === 'filter' && controlled ? '' : undefined}
        aria-label={label}
      >
        <Select.Value placeholder={placeholder} />
        <Select.Icon className="adw-popup-indicator">
          <Icon name="chevronDown" size={14} strokeWidth={2.2} />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal container={container}>
        {/* alignItemWithTrigger (default) overlays the selected item on the button. */}
        <Select.Positioner className="adw-positioner">
          <Select.Popup
            className="adw-menu adw-select-menu"
            style={{ minWidth: 'calc(var(--anchor-width) + 24px)' }}
          >
            <Select.List>
              {items.map(item => (
                <Select.Item key={item.value} value={item.value} className="adw-menu-item">
                  <span className="adw-menu-icon">
                    <Select.ItemIndicator>
                      <Icon name="check" size={14} strokeWidth={2.2} />
                    </Select.ItemIndicator>
                  </span>
                  <Select.ItemText className="adw-menu-label">{item.label}</Select.ItemText>
                </Select.Item>
              ))}
            </Select.List>
          </Select.Popup>
        </Select.Positioner>
      </Select.Portal>
    </Select.Root>
  )
}

export function Switch(props: ComponentProps<typeof BaseSwitch.Root>) {
  return (
    <BaseSwitch.Root className="adw-switch" {...props}>
      <BaseSwitch.Thumb className="adw-switch-thumb" />
    </BaseSwitch.Root>
  )
}

export function Checkbox({
  variant = 'default',
  ...props
}: { variant?: 'default' | 'plain' | 'on-selection' } & ComponentProps<typeof BaseCheckbox.Root>) {
  return (
    <BaseCheckbox.Root className="adw-checkbox" data-variant={variant} {...props}>
      <BaseCheckbox.Indicator>
        <Icon name="check" size={14} strokeWidth={3} />
      </BaseCheckbox.Indicator>
    </BaseCheckbox.Root>
  )
}

export function Badge({ children }: { children: ReactNode }) {
  return <span className="adw-badge">{children}</span>
}

/**
 * Pill-shaped tag; a palette `color` uses the design's tag tints, a custom `#rrggbb` is tinted from
 * that colour, and no colour is neutral.
 */
export function Tag({
  label,
  color,
  remove,
  detail,
  partial,
  action,
}: {
  label: string
  color?: TagColor
  /** The remove button's accessible name comes from the caller (localized). */
  remove?: { label: string; onClick: () => void }
  detail?: string
  /** Only some of the selection carries the tag: drawn striped . */
  partial?: boolean
  /** The tag itself is a button. */
  action?: { label: string; onClick: () => void }
}) {
  const content = (
    <>
      {label}
      {detail && <span className="adw-tag-detail">{detail}</span>}
    </>
  )
  return (
    <span
      className="adw-tag"
      data-tag={color && !isCustomColor(color) ? color : undefined}
      // 8-digit hex keeps the tint working on WebKit without color-mix() (Safari 15.6 target).
      style={color && isCustomColor(color) ? { background: `${color}29`, color } : undefined}
      data-removable={remove ? '' : undefined}
      data-partial={partial ? '' : undefined}
    >
      {action ? (
        <button
          type="button"
          className="adw-tag-action"
          aria-label={action.label}
          onClick={action.onClick}
        >
          {content}
        </button>
      ) : (
        content
      )}
      {remove && (
        <button
          type="button"
          className="adw-tag-remove"
          aria-label={remove.label}
          onClick={remove.onClick}
        >
          <Icon name="close" size={10} strokeWidth={2.4} />
        </button>
      )}
    </span>
  )
}
