import '@/components/ui/selection-item.css'
import type { KeyboardEvent } from 'react'
import type { DeviceRowStatus } from '@/components/device/device-trust-view'
import { getDeviceIcon } from '@/components/device/device-utils'
import StatusDot, { type StatusDotTone } from '@/components/device/StatusDot'
import SelectionIndicator from '@/components/ui/selection-indicator'
import { cn } from '@/lib/utils'

interface DeviceListItemProps {
  testId?: string
  name: string
  tone: StatusDotTone
  status: DeviceRowStatus
  selected: boolean
  dimmed?: boolean
  onSelect: () => void
}

function navigateDevices(event: KeyboardEvent<HTMLButtonElement>) {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
  const list = event.currentTarget.closest('[data-device-list]')
  if (!list) return
  const items = Array.from(list.querySelectorAll<HTMLButtonElement>('button[data-device-select]'))
  const current = items.indexOf(event.currentTarget)
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? items.length - 1
        : Math.max(0, Math.min(items.length - 1, current + (event.key === 'ArrowDown' ? 1 : -1)))
  event.preventDefault()
  items[next]?.focus()
  items[next]?.click()
}

export default function DeviceListItem({
  testId,
  name,
  tone,
  status,
  selected,
  dimmed,
  onSelect,
}: DeviceListItemProps) {
  const Icon = getDeviceIcon(name)
  return (
    <button
      type="button"
      data-testid={testId}
      data-device-select
      data-status={status.kind}
      aria-label={`${name} ${status.label}`}
      aria-current={selected ? 'true' : undefined}
      onClick={onSelect}
      onKeyDown={navigateDevices}
      title={name}
      className={cn(
        'selection-item relative isolate flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset',
        'text-foreground',
        dimmed && !selected && 'text-muted-foreground'
      )}
    >
      {selected && <SelectionIndicator layoutId="device-selection" className="bg-primary/10" />}
      <Icon
        className="selection-item-content relative z-10 size-5 shrink-0 text-muted-foreground"
        aria-hidden="true"
      />
      <span className="selection-item-content relative z-10 min-w-0 flex-1">
        <span className="block truncate text-sm font-medium">{name}</span>
        <span className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
          <StatusDot tone={tone} />
          <span className="min-w-0 break-words">{status.label}</span>
        </span>
      </span>
    </button>
  )
}
