import { Check, Monitor } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { presentDeviceGroups } from '@/components/device/device-group-presentation'
import { cn } from '@/lib/utils'

export function DeviceTrustChoiceCard({
  view,
  selected,
  disabled,
  tabStop,
  onSelect,
  showDetails,
}: {
  view: ReturnType<typeof presentDeviceGroups>['choices'][number]
  selected: boolean
  disabled: boolean
  tabStop: boolean
  onSelect: () => void
  showDetails: boolean
}) {
  const { t } = useTranslation()
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      tabIndex={tabStop ? 0 : -1}
      data-testid={`device-trust-choice-${view.id}`}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        'relative min-w-0 rounded-md border p-4 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60',
        selected ? 'border-primary bg-primary/5' : 'border-border hover:border-primary/50'
      )}
    >
      <span className="block pr-8 text-sm font-semibold break-words [overflow-wrap:anywhere]">
        {view.title}
      </span>
      <span
        aria-hidden="true"
        className={cn(
          'absolute top-4 right-4 flex size-5 items-center justify-center rounded-full border',
          selected ? 'border-primary bg-primary text-primary-foreground' : 'border-muted-foreground'
        )}
      >
        {selected && <Check className="size-3.5" />}
      </span>
      {view.summary && (
        <span className="mt-2 block pr-4 text-sm leading-6 text-muted-foreground [overflow-wrap:anywhere]">
          {view.summary}
        </span>
      )}
      <span hidden={!showDetails}>
        <span className="mt-4 flex items-start gap-2 border-t border-border pt-3 text-xs text-muted-foreground">
          <Monitor className="size-4 shrink-0" />
          {t('deviceTrust.presentation.members')}
        </span>
        <span
          data-testid="choice-members"
          className="mt-1 block text-sm leading-6 break-words [overflow-wrap:anywhere]"
        >
          {view.members}
        </span>
        {view.membersIncomplete && (
          <span className="mt-2 block text-xs text-muted-foreground">
            {t('deviceTrust.modal.membersIncomplete')}
          </span>
        )}
        <span className="mt-3 grid gap-2 text-xs leading-5 break-words [overflow-wrap:anywhere]">
          {view.scope && <span>{t('deviceTrust.presentation.scope', { names: view.scope })}</span>}
          {view.rejoin && (
            <span>{t('deviceTrust.presentation.rejoin', { names: view.rejoin })}</span>
          )}
        </span>
      </span>
      {!view.impactKnown && (
        <span className="mt-2 block text-xs text-muted-foreground">
          {t('deviceTrust.presentation.impactUnknown')}
        </span>
      )}
    </button>
  )
}
