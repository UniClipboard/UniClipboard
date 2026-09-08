import { ChevronDown, ChevronUp, Loader2, ShieldAlert } from 'lucide-react'
import { useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import type { DeviceGroupChoices } from '@/api/daemon/device-trust'
import { presentDeviceGroups } from '@/components/device/device-group-presentation'
import { DeviceTrustChoiceCard } from '@/components/device/DeviceTrustChoiceCard'
import { Button } from '@/components/ui/button'
import {
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

function moveChoice(event: KeyboardEvent<HTMLDivElement>) {
  if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return
  const options = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]:not(:disabled)')
  )
  const index = options.indexOf(document.activeElement as HTMLButtonElement)
  if (index < 0 || options.length < 2) return
  event.preventDefault()
  const offset = event.key === 'ArrowUp' || event.key === 'ArrowLeft' ? -1 : 1
  const next = options[(index + offset + options.length) % options.length]
  next.click()
  next.focus()
}

export function DeviceTrustDecisionContent({
  deviceGroups,
  busy,
  error,
  localRemovalConfirmationIssueId,
  onChoose,
  onRefresh,
  onBack,
  confirmationChoiceId,
}: {
  deviceGroups: DeviceGroupChoices
  busy: boolean
  error: string | null
  localRemovalConfirmationIssueId: string | null
  onChoose: (issueId: string, choiceId: string, confirmLocalRemoval: boolean) => void
  onRefresh?: () => void
  onBack?: () => void
  confirmationChoiceId?: string | null
}) {
  const { t } = useTranslation()
  const [selection, setSelection] = useState<string | null>(null)
  const [confirming, setConfirming] = useState(false)
  const [showDetails, setShowDetails] = useState(false)
  const issue = deviceGroups.issues[0]
  const view = presentDeviceGroups(deviceGroups, t)
  const selected = view.choices.find(choice => choice.id === (selection ?? confirmationChoiceId))
  const localConfirmation = confirming || localRemovalConfirmationIssueId === issue?.issueId
  if (!issue) return null
  const submit = () => {
    if (!selected || busy) return
    if (selected.removesLocal && !localConfirmation) {
      setConfirming(true)
      return
    }
    onChoose(issue.issueId, selected.id, localConfirmation)
  }
  return (
    <>
      <DialogHeader>
        <span className="flex items-start gap-3">
          <ShieldAlert className="mt-0.5 size-5 shrink-0 text-destructive" />
          <span className="min-w-0">
            <DialogTitle className="leading-6 [overflow-wrap:anywhere]">
              {view.localRemovalTitle ??
                t('deviceTrust.presentation.title', { name: view.localName })}
            </DialogTitle>
            <DialogDescription className={view.localRemovalTitle ? 'sr-only' : 'mt-2'}>
              {view.localRemovalTitle
                ? t('deviceTrust.presentation.select')
                : t('deviceTrust.presentation.local', { name: view.localName })}
            </DialogDescription>
          </span>
        </span>
        {!view.localRemovalTitle && (
          <p
            data-testid="choice-reason"
            className="mt-3 text-sm leading-6 [overflow-wrap:anywhere]"
          >
            {view.reason}
          </p>
        )}
        {view.detailsIncomplete && (
          <p className="text-xs text-muted-foreground">
            {t('deviceTrust.presentation.detailsIncomplete')}
          </p>
        )}
        {deviceGroups.issues.length > 1 && (
          <p className="text-xs text-muted-foreground">
            {t('deviceTrust.modal.issueProgress', {
              current: 1,
              total: deviceGroups.issues.length,
            })}
          </p>
        )}
      </DialogHeader>
      <DialogBody className="space-y-3 py-1">
        <div
          className="grid min-w-0 gap-3"
          role="radiogroup"
          aria-label={t('deviceTrust.presentation.select')}
          tabIndex={-1}
          onKeyDown={moveChoice}
        >
          {view.choices.map((choice, index) => (
            <DeviceTrustChoiceCard
              key={choice.id}
              view={choice}
              showDetails={showDetails}
              selected={selected?.id === choice.id}
              tabStop={selected ? selected.id === choice.id : index === 0}
              disabled={busy || localConfirmation}
              onSelect={() => {
                setSelection(choice.id)
                setConfirming(false)
              }}
            />
          ))}
        </div>
        {!localConfirmation && (
          <Button
            variant="ghost"
            size="sm"
            aria-expanded={showDetails}
            onClick={() => setShowDetails(value => !value)}
          >
            {showDetails ? <ChevronUp /> : <ChevronDown />}
            {t(`deviceTrust.presentation.${showDetails ? 'hideDevices' : 'showDevices'}`)}
          </Button>
        )}
        {error && (
          <p
            role="alert"
            data-testid="device-trust-error"
            data-error={error}
            className="text-sm text-destructive"
          >
            {t(
              error === 'device_state_changed'
                ? 'deviceTrust.presentation.changed'
                : 'deviceTrust.presentation.uncertain'
            )}
          </p>
        )}
        {localConfirmation && (
          <p data-testid="device-trust-local-removal-warning" className="text-sm text-destructive">
            {t('deviceTrust.modal.confirmLocalRemoval')}
          </p>
        )}
      </DialogBody>
      <DialogFooter className="flex-wrap">
        {localConfirmation && (
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => {
              setConfirming(false)
              setSelection(null)
              onBack?.()
            }}
          >
            {t('deviceTrust.presentation.back')}
          </Button>
        )}
        {error && onRefresh && (
          <Button variant="outline" onClick={onRefresh} disabled={busy}>
            {t('deviceTrust.presentation.retry')}
          </Button>
        )}
        <Button
          data-testid="device-trust-confirm"
          disabled={busy || !selected}
          onClick={submit}
          className="min-w-24 max-w-full whitespace-normal"
        >
          {busy && <Loader2 className="size-4 animate-spin" />}
          {localConfirmation
            ? t('deviceTrust.actions.confirmExit')
            : (selected?.title ?? t('deviceTrust.presentation.select'))}
        </Button>
      </DialogFooter>
    </>
  )
}
