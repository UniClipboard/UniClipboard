import { Check, Loader2, Plus, SignalHigh, Trash2, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { probeRelayUrl, type RelayProbeOutcome } from '@/api/tauri-command/settings'
import {
  Button,
  Input,
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui'

interface CustomRelayUrlsFieldProps {
  value: string[]
  onChange: (value: string[]) => void
}

type ProbeStatus =
  | { kind: 'idle' }
  | { kind: 'testing'; pendingUrl: string }
  | { kind: 'success'; latencyMs: number }
  | { kind: 'failure'; message: string }

const IDLE: ProbeStatus = { kind: 'idle' }

function visibleRows(value: string[]): string[] {
  return value.length > 0 ? value : ['']
}

function collapseSingleEmptyRow(value: string[]): string[] {
  return value.length === 1 && value[0].trim() === '' ? [] : value
}

let nextRowSeq = 0
const allocateRowKey = () => `relay-row-${++nextRowSeq}`

export function CustomRelayUrlsField({ value, onChange }: CustomRelayUrlsFieldProps) {
  const { t } = useTranslation()
  const rows = visibleRows(value)
  const canRemoveOnlyRow = value.length > 0
  const canAddRow = rows[rows.length - 1]?.trim() !== ''
  const [statuses, setStatuses] = useState<Record<number, ProbeStatus>>({})
  // Per-row stable identifiers — independent of array position so reordering or
  // removal doesn't reuse a sibling's React identity. Lazy-init lets the first
  // render see a length-matched list without touching state during render.
  const [rowKeys, setRowKeys] = useState<string[]>(() =>
    Array.from({ length: rows.length }, allocateRowKey)
  )
  if (rowKeys.length !== rows.length) {
    setRowKeys(previous => {
      if (previous.length === rows.length) return previous
      if (previous.length < rows.length) {
        const additions = Array.from({ length: rows.length - previous.length }, allocateRowKey)
        return [...previous, ...additions]
      }
      return previous.slice(0, rows.length)
    })
  }

  const clearStatusAt = (index: number) => {
    setStatuses(previous => {
      if (!(index in previous)) return previous
      const next = { ...previous }
      delete next[index]
      return next
    })
  }

  const handleRowChange = (index: number, nextValue: string) => {
    const next = [...rows]
    next[index] = nextValue
    onChange(collapseSingleEmptyRow(next))
    // Result is no longer trustworthy once the URL changes.
    clearStatusAt(index)
  }

  const handleRemoveRow = (index: number) => {
    if (!canRemoveOnlyRow && rows.length === 1) return
    onChange(collapseSingleEmptyRow(rows.filter((_, rowIndex) => rowIndex !== index)))
    // Drop the stable key for the removed row so the per-row identity tracks
    // the URL it was first bound to, not whatever happens to sit at the index.
    setRowKeys(previous => previous.filter((_, rowIndex) => rowIndex !== index))
    // Indices shift after removal; safest is to drop every cached status so
    // we never display a result against a different URL than the user tested.
    setStatuses({})
  }

  const handleAddRow = () => {
    if (!canAddRow) return
    onChange([...value, ''])
  }

  const handleTestRow = async (index: number, url: string) => {
    const trimmed = url.trim()
    if (trimmed.length === 0) return
    // Tag the probe with the URL it's testing. Either the user edited the row
    // (clearStatusAt wiped statuses[index]) or kicked off a second probe with
    // a different value while this one was in flight; in both cases the stale
    // response must NOT overwrite the visible state. We check that the slot
    // is still in `testing` for *this* exact URL before applying the outcome.
    setStatuses(previous => ({
      ...previous,
      [index]: { kind: 'testing', pendingUrl: trimmed },
    }))
    const isStillPending = (current: ProbeStatus | undefined): boolean =>
      current?.kind === 'testing' && current.pendingUrl === trimmed
    try {
      const outcome = await probeRelayUrl(trimmed)
      setStatuses(previous => {
        if (!isStillPending(previous[index])) return previous
        return { ...previous, [index]: outcomeToStatus(outcome, t) }
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      setStatuses(previous => {
        if (!isStillPending(previous[index])) return previous
        return {
          ...previous,
          [index]: {
            kind: 'failure',
            message: t('settings.sections.network.customRelays.testErrors.unavailable', {
              defaultValue: message,
            }),
          },
        }
      })
    }
  }

  return (
    <TooltipProvider delayDuration={200}>
      <div className="space-y-3 px-4 py-3">
        <div className="space-y-0.5">
          <label htmlFor="custom-relay-url-0" className="text-sm font-medium">
            {t('settings.sections.network.customRelays.label')}
          </label>
          <p className="text-xs leading-snug text-muted-foreground">
            {t('settings.sections.network.customRelays.description')}
          </p>
        </div>

        <div className="space-y-2">
          {rows.map((url, index) => {
            const status = statuses[index] ?? IDLE
            const canTest = url.trim().length > 0 && status.kind !== 'testing'
            const rowKey = rowKeys[index] ?? `relay-row-fallback-${index}`
            return (
              <div key={rowKey} className="flex min-w-0 items-center gap-2">
                <Input
                  id={index === 0 ? 'custom-relay-url-0' : undefined}
                  type="url"
                  inputMode="url"
                  autoComplete="off"
                  value={url}
                  placeholder={t('settings.sections.network.customRelays.placeholder')}
                  aria-label={t('settings.sections.network.customRelays.itemAriaLabel', {
                    index: index + 1,
                  })}
                  className="font-mono text-xs"
                  onChange={event => handleRowChange(index, event.target.value)}
                />
                <ProbeButton
                  status={status}
                  disabled={!canTest}
                  ariaLabel={t('settings.sections.network.customRelays.testAriaLabel', {
                    index: index + 1,
                  })}
                  onClick={() => handleTestRow(index, url)}
                  tooltip={statusTooltip(status, t)}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t('settings.sections.network.customRelays.removeAriaLabel', {
                    index: index + 1,
                  })}
                  disabled={!canRemoveOnlyRow && rows.length === 1}
                  onClick={() => handleRemoveRow(index)}
                >
                  <Trash2 aria-hidden="true" />
                </Button>
              </div>
            )
          })}
        </div>

        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={!canAddRow}
          onClick={handleAddRow}
        >
          <Plus aria-hidden="true" />
          {t('settings.sections.network.customRelays.addButton')}
        </Button>
      </div>
    </TooltipProvider>
  )
}

interface ProbeButtonProps {
  status: ProbeStatus
  disabled: boolean
  ariaLabel: string
  tooltip: string | null
  onClick: () => void
}

function ProbeButton({ status, disabled, ariaLabel, tooltip, onClick }: ProbeButtonProps) {
  const button = (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={onClick}
      data-probe-status={status.kind}
      className={
        status.kind === 'success'
          ? 'text-emerald-600 hover:text-emerald-700 dark:text-emerald-400'
          : status.kind === 'failure'
            ? 'text-destructive hover:text-destructive'
            : undefined
      }
    >
      {status.kind === 'testing' ? (
        <Loader2 aria-hidden="true" className="animate-spin" />
      ) : status.kind === 'success' ? (
        <Check aria-hidden="true" />
      ) : status.kind === 'failure' ? (
        <TriangleAlert aria-hidden="true" />
      ) : (
        <SignalHigh aria-hidden="true" />
      )}
    </Button>
  )

  if (!tooltip) return button
  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent side="top">{tooltip}</TooltipContent>
    </Tooltip>
  )
}

function statusTooltip(
  status: ProbeStatus,
  t: (key: string, opts?: Record<string, unknown>) => string
): string | null {
  switch (status.kind) {
    case 'testing':
      return t('settings.sections.network.customRelays.testing')
    case 'success':
      return t('settings.sections.network.customRelays.testSuccess', {
        latencyMs: status.latencyMs,
      })
    case 'failure':
      return status.message
    case 'idle':
      return null
  }
}

function outcomeToStatus(
  outcome: RelayProbeOutcome,
  t: (key: string, opts?: Record<string, unknown>) => string
): ProbeStatus {
  switch (outcome.kind) {
    case 'success':
      return { kind: 'success', latencyMs: outcome.latencyMs }
    case 'invalidUrl':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.invalidUrl', {
          message: outcome.message,
        }),
      }
    case 'dns':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.dns', {
          message: outcome.message,
        }),
      }
    case 'tls':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.tls', {
          message: outcome.message,
        }),
      }
    case 'handshake':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.handshake', {
          message: outcome.message,
        }),
      }
    case 'timeout':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.timeout'),
      }
    case 'other':
      return {
        kind: 'failure',
        message: t('settings.sections.network.customRelays.testErrors.other', {
          message: outcome.message,
        }),
      }
  }
}
