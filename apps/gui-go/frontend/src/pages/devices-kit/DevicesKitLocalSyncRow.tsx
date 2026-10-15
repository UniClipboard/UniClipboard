import { useLocalSyncToggle } from '@/hooks/useLocalSyncToggle'
import { SettingsRow, Switch } from '@/ui-kit'

/** One of this device's sync switches (all sync, file sync). */
export function DevicesKitLocalSyncRow({ kind }: { kind: 'sync' | 'file' }) {
  const toggle = useLocalSyncToggle(kind)
  return (
    <SettingsRow label={toggle.title} description={toggle.description}>
      <Switch
        label={toggle.title}
        labelHidden
        checked={toggle.checked}
        disabled={!toggle.available || toggle.saving}
        onCheckedChange={value => void toggle.save(value)}
      />
    </SettingsRow>
  )
}
