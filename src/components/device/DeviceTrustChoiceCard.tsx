import { Check, Link2, Unlink } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { DeviceTrustOutcomeRow } from '@/components/device/DeviceTrustOutcomeRow'
import { cn } from '@/lib/utils'

export function DeviceTrustChoiceCard({
  title,
  continuesWith,
  stopsWith,
  selected,
  disabled,
  onSelect,
}: {
  title: string
  continuesWith: string
  stopsWith: string
  selected: boolean
  disabled: boolean
  onSelect: () => void
}) {
  const { t } = useTranslation()
  return (
    <button
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
    </button>
  )
}
