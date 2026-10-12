import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
} from '@/components/ui'
import type { SettingSelectProps, SettingSwitchProps } from './types'

export function SettingSwitch({
  checked,
  onCheckedChange,
  label,
  disabled,
  id,
}: SettingSwitchProps) {
  return (
    <Switch
      id={id}
      aria-label={label}
      checked={checked}
      onCheckedChange={onCheckedChange}
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
  const selected = items.find(item => item.value === value)
  const described = items.some(item => item.description)
  return (
    <div className="w-40">
      <Select value={value} onValueChange={onValueChange} disabled={disabled}>
        <SelectTrigger aria-label={label} className="w-full">
          <SelectValue>{selected?.label}</SelectValue>
        </SelectTrigger>
        <SelectContent className={described ? 'w-72 max-w-[calc(100vw-2rem)]' : undefined}>
          {items.map(item => (
            <SelectItem key={item.value} value={item.value} aria-label={item.label}>
              {item.description ? (
                <span className="flex min-w-0 flex-col gap-1">
                  <span>{item.label}</span>
                  <span className="text-ui-caption-relaxed text-muted-foreground">
                    {item.description}
                  </span>
                </span>
              ) : (
                item.label
              )}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}
