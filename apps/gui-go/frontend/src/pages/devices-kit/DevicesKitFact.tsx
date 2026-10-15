import type { ReactNode } from 'react'
import { SettingsRow, Text } from '@/ui-kit'

/** One read-only fact of a device (peer ID, address, platform) as a settings row. */
export function DevicesKitFact({
  label,
  value,
  monospace,
  trailing,
}: {
  label: string
  value: string
  monospace?: boolean
  trailing?: ReactNode
}) {
  return (
    <SettingsRow label={label}>
      <Text role="callout" tone="secondary" monospace={monospace} truncate>
        {value}
      </Text>
      {trailing}
    </SettingsRow>
  )
}
