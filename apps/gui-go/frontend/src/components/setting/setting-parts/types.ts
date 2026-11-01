import type { ReactNode } from 'react'

export interface SettingActionButtonProps {
  onClick: () => void
  disabled?: boolean
  children: ReactNode
}

export interface SettingGroupProps {
  title?: string
  children: ReactNode
  className?: string
}

export interface SettingRowProps {
  label?: string
  labelExtra?: ReactNode
  description?: string
  children?: ReactNode
  className?: string
  /**
   * Data-driven experimental marker. When the key is registered in
   * `experimental-features.ts`, an ExperimentalBadge is rendered next to the label.
   */
  experimentalKey?: string
}

export interface SettingSwitchProps {
  checked: boolean
  onCheckedChange?: (checked: boolean) => void
  /** Accessible name of the switch. */
  label: string
  disabled?: boolean
  id?: string
}

export interface SettingSelectItem {
  value: string
  label: string
  /** Second line in the open list, where the skin has room for one. */
  description?: string
}

export interface SettingSelectProps {
  items: SettingSelectItem[]
  value: string
  onValueChange: (value: string) => void
  /** Accessible name of the pop-up. */
  label: string
  disabled?: boolean
}

export interface SettingAlertProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description?: string
  /** Content under the description. */
  children?: ReactNode
  confirmLabel: string
  cancelLabel: string
  /** The confirmed action destroys data. */
  destructive?: boolean
  /**
   * A synchronous confirmation closes the alert; one returning a promise keeps it open and the
   * caller closes it.
   */
  onConfirm: () => void | Promise<void>
  /** The Cancel button was used (not Escape or a click outside). */
  onCancel?: () => void
  confirmDisabled?: boolean
  /** The confirmed work is running: shown on the confirm button, and the alert cannot be dismissed. */
  busyLabel?: string
  /** The existing alert drops its buttons while busy and shows the busy label alone. */
  busyReplacesActions?: boolean
  /** Leading icon of the existing alert; the platform kit draws its own. */
  media?: ReactNode
  /** Keyboard hints inside the existing alert's buttons; the platform kit has none. */
  cancelHint?: ReactNode
  confirmHint?: ReactNode
}
