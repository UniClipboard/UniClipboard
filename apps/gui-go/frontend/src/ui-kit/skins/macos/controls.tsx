import { Toggle } from '@base-ui/react/toggle'
import { Tooltip } from '@base-ui/react/tooltip'
import type { ReactElement } from 'react'
import type {
  BadgeProps,
  ButtonProps,
  ButtonVariant,
  CheckboxProps,
  CheckedControlProps,
  ControlSize,
  InlineNoticeProps,
  SearchFieldProps,
  SearchTokenProps,
  SegmentedControlProps,
  SelectProps,
  SwitchProps,
  TagProps,
  ToggleButtonProps,
} from '@/ui-kit/contract'
import { glyph } from './glyph'
import { Icon } from './icons'
import * as parts from './parts'
import { usePortalContainer } from './portal'

/** `subtle` has no bezel at rest, which on macOS is a borderless button. */
const tintOf: Record<ButtonVariant, 'primary' | 'none' | 'borderless' | 'destructive'> = {
  primary: 'primary',
  standard: 'none',
  subtle: 'borderless',
  destructive: 'destructive',
}

/** AppKit help tag: shown on hover after a delay; the trigger keeps its own accessible name. */
export function WithHelpTag({ text, children }: { text?: string; children: ReactElement }) {
  const container = usePortalContainer()
  if (!text) return children
  return (
    <Tooltip.Root>
      <Tooltip.Trigger render={children} />
      <Tooltip.Portal container={container}>
        <Tooltip.Positioner className="mac-positioner" sideOffset={8}>
          <Tooltip.Popup className="mac-tooltip">{text}</Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

export function Button({
  variant = 'standard',
  size = 'medium',
  icon,
  children,
  label,
  tooltip,
  disabled,
  onClick,
}: ButtonProps) {
  const iconOnly = children === undefined || children === null
  return (
    <WithHelpTag text={tooltip ?? (iconOnly ? label : undefined)}>
      <parts.PushButton
        size={size}
        tint={tintOf[variant]}
        icon={icon && glyph(icon)}
        aria-label={iconOnly ? label : undefined}
        disabled={disabled}
        onClick={onClick}
        data-icon-only={iconOnly ? '' : undefined}
      >
        {children}
      </parts.PushButton>
    </WithHelpTag>
  )
}

const iconSize = (size: ControlSize) => (size === 'small' ? 12 : size === 'medium' ? 14 : 16)

export function ToggleButton({
  pressed,
  defaultPressed,
  onPressedChange,
  size = 'medium',
  icon,
  children,
  label,
  tooltip,
  disabled,
}: ToggleButtonProps) {
  const iconOnly = children === undefined || children === null
  return (
    <WithHelpTag text={tooltip ?? (iconOnly ? label : undefined)}>
      <Toggle
        className="mac-btn"
        data-size={size}
        data-tint="none"
        data-icon-only={iconOnly ? '' : undefined}
        pressed={pressed}
        defaultPressed={defaultPressed}
        onPressedChange={next => onPressedChange?.(next)}
        aria-label={iconOnly ? label : undefined}
        disabled={disabled}
      >
        {icon && <Icon name={glyph(icon)} size={iconSize(size)} />}
        {children}
      </Toggle>
    </WithHelpTag>
  )
}

function Labelled({
  label,
  labelHidden,
  children,
}: Pick<CheckedControlProps, 'label' | 'labelHidden'> & { children: ReactElement }) {
  if (labelHidden) return children
  return (
    <label className="mac-check-label">
      {children}
      <span>{label}</span>
    </label>
  )
}

export function Switch({
  checked,
  defaultChecked,
  onCheckedChange,
  label,
  labelHidden,
  disabled,
}: SwitchProps) {
  return (
    <Labelled label={label} labelHidden={labelHidden}>
      <parts.Switch
        checked={checked}
        defaultChecked={defaultChecked}
        onCheckedChange={next => onCheckedChange?.(next)}
        aria-label={labelHidden ? label : undefined}
        disabled={disabled}
      />
    </Labelled>
  )
}

export function Checkbox({
  checked,
  defaultChecked,
  onCheckedChange,
  label,
  labelHidden,
  disabled,
}: CheckboxProps) {
  return (
    <Labelled label={label} labelHidden={labelHidden}>
      <parts.Checkbox
        checked={checked}
        defaultChecked={defaultChecked}
        onCheckedChange={next => onCheckedChange?.(next)}
        aria-label={labelHidden ? label : undefined}
        disabled={disabled}
      />
    </Labelled>
  )
}

export function SearchField({
  value,
  defaultValue,
  onValueChange,
  placeholder,
  label,
  shortcutHint,
  onSubmit,
  inputRef,
}: SearchFieldProps) {
  return (
    <parts.SearchField
      label={label}
      placeholder={placeholder}
      hint={shortcutHint ?? ''}
      value={value}
      defaultValue={defaultValue}
      inputRef={inputRef}
      onValueChange={next => onValueChange?.(next)}
      onKeyDown={event => {
        if (event.key === 'Enter' && onSubmit) {
          event.preventDefault()
          onSubmit(event.currentTarget.value)
        }
      }}
    />
  )
}

export function Select(props: SelectProps) {
  return <parts.PopUpButton {...props} />
}

export function SegmentedControl({
  items,
  value,
  defaultValue,
  onValueChange,
  label,
}: SegmentedControlProps) {
  return (
    <parts.Segmented
      label={label}
      value={value}
      defaultValue={defaultValue}
      onValueChange={onValueChange}
      items={items.map(item => ({ ...item, icon: item.icon && glyph(item.icon) }))}
    />
  )
}

export function Badge({ count, label }: BadgeProps) {
  return (
    <span className="mac-badge" role="img" aria-label={label}>
      {count}
    </span>
  )
}

export function Tag({ name, color, remove, detail, partial, action }: TagProps) {
  return (
    <parts.Tag
      label={name}
      color={color}
      remove={remove}
      detail={detail}
      partial={partial}
      action={action}
    />
  )
}

export function SearchToken({ field, value, remove }: SearchTokenProps) {
  // `field` carries its own colon (`from:`), as in the Windows skin.
  const text = `${field} ${value}`
  return (
    <span className="mac-token">
      {text}
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

const noticeStyle = {
  info: { tone: 'blue', icon: 'info' },
  success: { tone: 'green', icon: 'checkCircle' },
  warning: { tone: 'orange', icon: 'warning' },
  error: { tone: 'red', icon: 'xCircle' },
} as const

/** macOS has no InfoBar; the design uses a tinted rounded block. */
export function InlineNotice({ tone, title, text, action, dismiss }: InlineNoticeProps) {
  const style = noticeStyle[tone]
  return (
    <div className="mac-notice" data-tone={style.tone} role={tone === 'error' ? 'alert' : 'status'}>
      <Icon name={style.icon} size={16} color={`var(--${style.tone})`} />
      <span className="mac-notice-title">{title}</span>
      {text && <span className="mac-notice-text">{text}</span>}
      {(action || dismiss) && (
        <span className="mac-notice-action">
          {action && (
            <parts.PushButton size="small" onClick={action.onClick}>
              {action.label}
            </parts.PushButton>
          )}
          {dismiss && (
            <button
              type="button"
              className="mac-tag-remove"
              aria-label={dismiss.label}
              onClick={dismiss.onClick}
            >
              <Icon name="close" size={10} strokeWidth={2.4} />
            </button>
          )}
        </span>
      )}
    </div>
  )
}
