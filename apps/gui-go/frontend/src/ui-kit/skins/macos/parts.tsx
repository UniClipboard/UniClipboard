import { Button } from '@base-ui/react/button'
import { Checkbox as BaseCheckbox } from '@base-ui/react/checkbox'
import { Input } from '@base-ui/react/input'
import { Radio } from '@base-ui/react/radio'
import { RadioGroup } from '@base-ui/react/radio-group'
import { Select } from '@base-ui/react/select'
import { Switch as BaseSwitch } from '@base-ui/react/switch'
import { createContext, use, useState, type ComponentProps, type ReactNode } from 'react'
import { Icon, type IconName } from './icons'

/** Popups portal into the themed surface that owns them, so a dark panel's menu keeps dark tokens. */
export const PortalContainer = createContext<HTMLElement | null>(null)

export function usePortalContainer() {
  return use(PortalContainer)
}

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
  tint?: 'none' | 'primary' | 'secondary' | 'destructive' | 'glass' | 'borderless'
  icon?: IconName
  force?: ForcedState
} & ComponentProps<typeof Button>) {
  const iconSize = size === 'medium' ? 14 : size === 'small' ? 12 : 16
  return (
    <Button
      className="mac-btn"
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

export function GlassItem({
  icon,
  label,
  iconSize = 16,
  strokeWidth = 1.7,
  ...props
}: {
  icon: IconName
  label: string
  iconSize?: number
  strokeWidth?: number
} & ComponentProps<'button'>) {
  return (
    <button type="button" className="mac-glass-item" aria-label={label} {...props}>
      <Icon name={icon} size={iconSize} strokeWidth={strokeWidth} />
    </button>
  )
}

export function SearchField({
  width = '100%',
  placeholder = 'Search',
  label = placeholder,
  hint = '⌘F',
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
    <label className="mac-search mac-glass" style={{ width }} data-force={force}>
      <Icon name="search" size={14} />
      {tokens?.map(token => (
        <span key={token} className="mac-token">
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
 * NSSegmentedControl in select-one mode: a toolbar holding a radio group, so arrow keys move and
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
    <div role="toolbar" aria-label={label} className="mac-segmented">
      <RadioGroup
        aria-label={label}
        className="mac-segmented-group"
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
            className="mac-segment"
            aria-label={item.iconOnly ? item.label : undefined}
            title={item.iconOnly ? item.label : undefined}
          >
            {item.icon && <Icon name={item.icon} size={13} />}
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
}: {
  items: { value: string; label: string }[]
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  label: string
  placeholder?: string
  disabled?: boolean
}) {
  const container = usePortalContainer()
  return (
    <Select.Root
      items={items}
      value={value}
      defaultValue={defaultValue}
      onValueChange={next => {
        if (next !== null) onValueChange?.(next)
      }}
      disabled={disabled}
      modal={false}
    >
      <Select.Trigger className="mac-popup-btn" aria-label={label}>
        <Select.Value placeholder={placeholder} />
        <Select.Icon className="mac-popup-indicator">
          <Icon name="chevronDown" size={10} strokeWidth={2.4} />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal container={container}>
        {/* alignItemWithTrigger (default) overlays the selected item on the button, like an AppKit pop-up. */}
        <Select.Positioner className="mac-positioner">
          <Select.Popup
            className="mac-menu"
            style={{ minWidth: 'calc(var(--anchor-width) + 24px)' }}
          >
            <Select.List>
              {items.map(item => (
                <Select.Item key={item.value} value={item.value} className="mac-menu-item">
                  <span className="mac-menu-icon">
                    <Select.ItemIndicator>
                      <Icon name="check" size={12} strokeWidth={2.4} />
                    </Select.ItemIndicator>
                  </span>
                  <Select.ItemText className="mac-menu-label">{item.label}</Select.ItemText>
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
    <BaseSwitch.Root className="mac-switch" {...props}>
      <BaseSwitch.Thumb className="mac-switch-thumb" />
    </BaseSwitch.Root>
  )
}

export function Checkbox({
  variant = 'default',
  ...props
}: { variant?: 'default' | 'plain' | 'on-selection' } & ComponentProps<typeof BaseCheckbox.Root>) {
  return (
    <BaseCheckbox.Root className="mac-checkbox" data-variant={variant} {...props}>
      <BaseCheckbox.Indicator>
        <Icon name="check" size={11} strokeWidth={2.8} />
      </BaseCheckbox.Indicator>
    </BaseCheckbox.Root>
  )
}

export function Badge({ children }: { children: ReactNode }) {
  return <span className="mac-badge">{children}</span>
}

/** Capsule tag; `color` picks the project tag palette (design T1), anything else is neutral. */
export function Tag({
  label,
  color,
  remove,
}: {
  label: string
  color?: string
  /** The remove button's accessible name comes from the caller (localized). */
  remove?: { label: string; onClick: () => void }
}) {
  return (
    <span className="mac-tag" data-tag={color} data-removable={remove ? '' : undefined}>
      {label}
      {remove && (
        <button
          type="button"
          className="mac-tag-remove"
          aria-label={remove.label}
          onClick={remove.onClick}
        >
          <Icon name="close" size={9} strokeWidth={2.4} />
        </button>
      )}
    </span>
  )
}
