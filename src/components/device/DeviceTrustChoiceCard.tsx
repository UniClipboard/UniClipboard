import { AlertTriangle, Check, Link2, Unlink } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { DeviceTrustOutcomeRow } from '@/components/device/DeviceTrustOutcomeRow'
import { cn } from '@/lib/utils'

export function DeviceTrustChoiceCard({
  testId,
  title,
  continuesWith,
  stopsWith,
  selected,
  disabled,
  onSelect,
  note,
}: {
  testId?: string
  title: string
  continuesWith: string
  stopsWith: string
  selected: boolean
  disabled: boolean
  onSelect: () => void
  note?: string
}) {
  const { t } = useTranslation()
  return (
    <button
      data-testid={testId}
      type="button"
      role="radio"
      aria-checked={selected}
      tabIndex={selected ? 0 : -1}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        'group relative min-w-0 rounded-md border p-4 text-left outline-none transition-colors',
        'hover:border-primary/50 hover:bg-primary/5 focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50',
        'disabled:cursor-not-allowed disabled:opacity-50',
        selected ? 'border-primary bg-primary/5 ring-1 ring-primary/20' : 'border-border bg-card'
      )}
    >
      <span
        className={cn(
          'absolute top-3 right-3 flex size-5 items-center justify-center rounded-full border transition-colors',
          selected
            ? 'border-primary bg-primary text-primary-foreground'
            : 'border-muted-foreground/40 bg-background group-hover:border-primary/60'
        )}
        aria-hidden="true"
      >
        {selected && <Check className="size-3.5" strokeWidth={3} />}
      </span>
      <span className="block pr-7 text-sm font-semibold">{title}</span>
      <span className="mt-3 grid gap-2">
        <DeviceTrustOutcomeRow
          icon={Link2}
          label={t('deviceTrust.modal.continueSyncing')}
          devices={continuesWith}
          tone="success"
        />
        <DeviceTrustOutcomeRow
          icon={Unlink}
          label={t('deviceTrust.modal.stopSyncing')}
          devices={stopsWith}
          tone="danger"
        />
      </span>
      {note && (
        <span className="mt-3 flex items-start gap-2 border-t border-border/70 pt-3 text-xs font-medium text-amber-700 dark:text-amber-300">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span>{note}</span>
        </span>
      )}
    </button>
  )
}
