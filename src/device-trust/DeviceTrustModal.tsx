import { Check, Link2, Loader2, ShieldAlert, Unlink } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DeviceTrustChoice, DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { getPendingDecisionView } from './device-trust-model'

export function DeviceTrustModal({
  snapshot,
  busy,
  error,
  onDecide,
}: {
  snapshot: DeviceTrustSnapshot
  busy: boolean
  error: string | null
  onDecide: (choice: DeviceTrustChoice, confirmLocalRemoval: boolean) => void
}) {
  const { t } = useTranslation()
  const view = useMemo(() => getPendingDecisionView(snapshot), [snapshot])
  const [choice, setChoice] = useState<DeviceTrustChoice>('apply_change')
  if (!view || !snapshot.currentChange) return null

  const names = (items: typeof view.targets) =>
    items.length > 0
      ? items.map(item => item.label).join(t('deviceTrust.listSeparator'))
      : t('deviceTrust.modal.noDevices')

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/25 p-4 backdrop-blur-xs"
      role="presentation"
    >
      <section
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="device-trust-title"
        aria-describedby="device-trust-description"
        className="max-h-[calc(100vh-2rem)] min-w-0 w-full max-w-xl overflow-y-auto rounded-lg border border-border bg-card text-card-foreground shadow-2xl"
      >
        <header className="flex items-start gap-3 border-b border-border px-5 py-4">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-md bg-destructive/10 text-destructive">
            <ShieldAlert className="size-5" aria-hidden="true" />
          </span>
          <div className="min-w-0">
            <h2 id="device-trust-title" className="text-base font-semibold">
              {t('deviceTrust.modal.title')}
            </h2>
            <p id="device-trust-description" className="mt-1 text-sm text-muted-foreground">
              {view.includesLocalDevice
                ? t('deviceTrust.modal.localSummary', { proposer: view.proposer.label })
                : t('deviceTrust.modal.summary', {
                    proposer: view.proposer.label,
                    targets: names(view.targets),
                  })}
            </p>
          </div>
        </header>

        <div className="space-y-4 px-5 py-4">
          <div className="grid min-w-0 gap-3" role="radiogroup">
            <ChoiceCard
              selected={choice === 'apply_change'}
              disabled={busy}
              onSelect={() => setChoice('apply_change')}
              title={t(
                view.includesLocalDevice
                  ? 'deviceTrust.modal.leaveTitle'
                  : 'deviceTrust.modal.applyTitle'
              )}
              continuesWith={names(view.apply.continuesWith)}
              stopsWith={names(view.apply.stopsWith)}
            />
            <ChoiceCard
              selected={choice === 'keep_current_device_group'}
              disabled={busy}
              onSelect={() => setChoice('keep_current_device_group')}
              title={t(
                view.includesLocalDevice
                  ? 'deviceTrust.modal.stayTitle'
                  : 'deviceTrust.modal.keepTitle'
              )}
              continuesWith={names(view.keepCurrent.continuesWith)}
              stopsWith={names(view.keepCurrent.stopsWith)}
            />
          </div>
          {error && (
            <p className="rounded-md border border-destructive/20 bg-destructive/10 px-3 py-2 text-sm font-medium text-destructive">
              {error === 'device_state_changed'
                ? t('deviceTrust.modal.stateChanged')
                : t('deviceTrust.modal.failed')}
            </p>
          )}
        </div>

        <footer className="flex justify-end border-t border-border bg-muted/40 px-5 py-4">
          <Button
            className="min-w-24"
            disabled={busy}
            onClick={() => onDecide(choice, choice === 'apply_change' && view.includesLocalDevice)}
          >
            {busy && <Loader2 className="animate-spin" aria-hidden="true" />}
            {t('deviceTrust.actions.confirm')}
          </Button>
        </footer>
      </section>
    </div>
  )
}

function ChoiceCard({
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
        <OutcomeRow
          icon={Link2}
          label={t('deviceTrust.modal.continueSyncing')}
          devices={continuesWith}
          tone="success"
        />
        <OutcomeRow
          icon={Unlink}
          label={t('deviceTrust.modal.stopSyncing')}
          devices={stopsWith}
          tone="danger"
        />
      </span>
    </button>
  )
}

function OutcomeRow({
  icon: Icon,
  label,
  devices,
  tone,
}: {
  icon: typeof Link2
  label: string
  devices: string
  tone: 'success' | 'danger'
}) {
  return (
    <span
      className={cn(
        'grid min-w-0 grid-cols-[1rem_5rem_minmax(0,1fr)] items-start gap-2 text-xs leading-5',
        tone === 'success' ? 'text-emerald-600 dark:text-emerald-400' : 'text-destructive'
      )}
    >
      <Icon className="mt-0.5 size-4" aria-hidden="true" />
      <span>{label}</span>
      <span className="break-words font-medium">{devices}</span>
    </span>
  )
}
