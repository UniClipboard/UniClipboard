import { isExperimentalFeature } from '@/components/setting/experimental-features'
import { ExperimentalBadge } from '@/components/setting/ExperimentalBadge'
import { AlertDialog, Button, Select, SettingsGroup, SettingsRow, Switch } from '@/ui-kit'
import type {
  SettingAlertProps,
  SettingButtonProps,
  SettingGroupProps,
  SettingRowProps,
  SettingSelectProps,
  SettingSwitchProps,
} from './types'

// Platform-kit view of the setting parts (macOS build, `@ui-setting-parts`). The sections keep
// every value, save and restart rule; only the presentation comes from the skin.

export function SettingGroup({ title, children }: SettingGroupProps) {
  return <SettingsGroup title={title}>{children}</SettingsGroup>
}

export function SettingRow({
  label,
  labelExtra,
  description,
  children,
  experimentalKey,
}: SettingRowProps) {
  const experimental = isExperimentalFeature(experimentalKey)
  return (
    <SettingsRow
      label={label}
      labelExtra={
        experimental || labelExtra ? (
          <>
            {experimental && <ExperimentalBadge />}
            {labelExtra}
          </>
        ) : undefined
      }
      description={description}
    >
      {children}
    </SettingsRow>
  )
}

export function SettingSwitch({ checked, onCheckedChange, label, disabled }: SettingSwitchProps) {
  return (
    <Switch
      checked={checked}
      onCheckedChange={onCheckedChange}
      label={label}
      labelHidden
      disabled={disabled}
    />
  )
}

export function SettingSelect({
  items,
  value,
  onValueChange,
  label,
  disabled,
}: SettingSelectProps) {
  return (
    <Select
      items={items.map(({ value: itemValue, label: itemLabel }) => ({
        value: itemValue,
        label: itemLabel,
      }))}
      value={value}
      onValueChange={onValueChange}
      label={label}
      disabled={disabled}
    />
  )
}

export function SettingAlert({
  media: _media,
  busyReplacesActions: _replaces,
  cancelHint: _cancelHint,
  confirmHint: _confirmHint,
  ...alert
}: SettingAlertProps) {
  return <AlertDialog {...alert} />
}

export { InlineNotice as SettingNotice } from '@/ui-kit'
export { SettingPresentationRoot } from './kit-root'

export function SettingButton(props: SettingButtonProps) {
  return <Button variant="standard" size="small" {...props} />
}
