import { Loader2, ShieldAlert } from 'lucide-react'
import { useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  DeviceGroupChoice,
  DeviceGroupChoices,
  DeviceTrustRelationship,
} from '@/api/daemon/device-trust'
import { getDeviceLabel, getPendingDecisionView } from '@/components/device/device-trust-model'
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
  const currentIndex = options.indexOf(document.activeElement as HTMLButtonElement)
  if (currentIndex === -1 || options.length < 2) return
  event.preventDefault()
  const offset = event.key === 'ArrowUp' || event.key === 'ArrowLeft' ? -1 : 1
  const next = options[(currentIndex + offset + options.length) % options.length]
  next.click()
  next.focus()
}

function deviceDisplayMap(devices: DeviceTrustRelationship[]) {
  const nameCounts = new Map<string, number>()
  for (const device of devices) {
    const name = device.displayName.trim().toLocaleLowerCase()
    if (name) nameCounts.set(name, (nameCounts.get(name) ?? 0) + 1)
  }
  return new Map(
    devices.map(device => {
      const displayName = device.displayName.trim()
      const duplicate =
        displayName.length > 0 && (nameCounts.get(displayName.toLocaleLowerCase()) ?? 0) > 1
      return [
        device.deviceId,
        displayName && !duplicate ? displayName : getDeviceLabel(displayName, device.deviceId),
      ] as const
    })
  )
}

export function DeviceTrustDecisionContent({
  deviceGroups,
  busy,
  error,
  localRemovalConfirmationIssueId,
  onChoose,
}: {
  deviceGroups: DeviceGroupChoices
  busy: boolean
  error: string | null
  localRemovalConfirmationIssueId: string | null
  onChoose: (issueId: string, choiceId: string, confirmLocalRemoval: boolean) => void
}) {
  const { t } = useTranslation()
  const issue = deviceGroups.issues[0]
  const choices = issue?.choices ?? []
  const [selectedChoiceId, setSelectedChoiceId] = useState(() => choices[0]?.choiceId ?? '')
  if (!issue || choices.length === 0) return null

  const snapshot = deviceGroups.deviceTrust
  const pendingView = getPendingDecisionView(snapshot)
  const selectedChoice = choices.find(choice => choice.choiceId === selectedChoiceId) ?? choices[0]
  const confirmingLocalRemoval =
    localRemovalConfirmationIssueId === issue.issueId &&
    !selectedChoice.memberDeviceIds.includes(snapshot.localDeviceId)
  const labels = deviceDisplayMap(snapshot.devices)
  const allPeerIds = snapshot.devices.flatMap(device =>
    device.isLocal || device.membership === 'removed' ? [] : [device.deviceId]
  )
  const names = (deviceIds: string[]) => {
    const values = deviceIds.flatMap(deviceId =>
      deviceId === snapshot.localDeviceId
        ? []
        : [labels.get(deviceId) ?? getDeviceLabel('', deviceId)]
    )
    return values.length > 0
      ? values.join(t('deviceTrust.listSeparator'))
      : t('deviceTrust.modal.noDevices')
  }
  const cardView = (choice: DeviceGroupChoice) => {
    if (pendingView && choice.choiceId === 'apply') {
      return {
        title: t(
          pendingView.includesLocalDevice
            ? 'deviceTrust.modal.leaveTitle'
            : 'deviceTrust.modal.applyTitle'
        ),
        continuesWith: names(pendingView.apply.continuesWith.map(device => device.deviceId)),
        stopsWith: names(pendingView.apply.stopsWith.map(device => device.deviceId)),
      }
    }
    if (pendingView && choice.choiceId === 'keep') {
      return {
        title: t(
          pendingView.includesLocalDevice
            ? 'deviceTrust.modal.stayTitle'
            : 'deviceTrust.modal.keepTitle'
        ),
        continuesWith: names(pendingView.keepCurrent.continuesWith.map(device => device.deviceId)),
        stopsWith: names(pendingView.keepCurrent.stopsWith.map(device => device.deviceId)),
      }
    }
    const memberIds = new Set(choice.memberDeviceIds)
    return {
      title: t(
        choice.isCurrentGroup ? 'deviceTrust.modal.stayTitle' : 'deviceTrust.modal.useGroupTitle'
      ),
      continuesWith: names(choice.memberDeviceIds),
      stopsWith: names(allPeerIds.filter(deviceId => !memberIds.has(deviceId))),
    }
  }

  return (
    <>
      <DialogHeader className="flex-row items-start gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-md bg-destructive/10 text-destructive">
          <ShieldAlert className="size-5" aria-hidden="true" />
        </span>
        <span className="min-w-0">
          <DialogTitle>{t('deviceTrust.modal.title')}</DialogTitle>
          <DialogDescription className="mt-1">
            {pendingView
              ? pendingView.includesLocalDevice
                ? t('deviceTrust.modal.localSummary', { proposer: pendingView.proposer.label })
                : t('deviceTrust.modal.summary', {
                    proposer: pendingView.proposer.label,
                    targets: names(pendingView.targets.map(device => device.deviceId)),
                  })
              : t('deviceTrust.modal.groupConflictSummary')}
          </DialogDescription>
          {deviceGroups.issues.length > 1 && (
            <p className="mt-1 text-xs text-muted-foreground">
              {t('deviceTrust.modal.issueProgress', {
                current: 1,
                total: deviceGroups.issues.length,
              })}
            </p>
          )}
        </span>
      </DialogHeader>
      <DialogBody className="space-y-4 py-1">
        <div className="grid min-w-0 gap-3" role="radiogroup" onKeyDown={moveChoice}>
          {choices.map(choice => {
            const view = cardView(choice)
            const notes = [
              choice.requiresRePairing ? t('deviceTrust.modal.requiresRePairing') : null,
              !choice.membersComplete ? t('deviceTrust.modal.membersIncomplete') : null,
            ].filter((note): note is string => note !== null)
            return (
              <DeviceTrustChoiceCard
                key={choice.choiceId}
                selected={selectedChoice.choiceId === choice.choiceId}
                disabled={busy}
                onSelect={() => setSelectedChoiceId(choice.choiceId)}
                title={view.title}
                continuesWith={view.continuesWith}
                stopsWith={view.stopsWith}
                note={notes.join(' ')}
              />
            )
          })}
        </div>
        {error && (
          <p className="rounded-md border border-destructive/20 bg-destructive/10 px-3 py-2 text-sm font-medium text-destructive">
            {error === 'device_state_changed'
              ? t('deviceTrust.modal.stateChanged')
              : error === 'choice_pending'
                ? t('deviceTrust.modal.choicePending')
                : error === 're_pairing_required'
                  ? t('deviceTrust.modal.rePairingRequired')
                  : t('deviceTrust.modal.failed')}
          </p>
        )}
        {confirmingLocalRemoval && (
          <p className="rounded-md border border-destructive/20 bg-destructive/10 px-3 py-2 text-sm font-medium text-destructive">
            {t('deviceTrust.modal.confirmLocalRemoval')}
          </p>
        )}
      </DialogBody>
      <DialogFooter>
        <Button
          className="min-w-24"
          disabled={busy}
          onClick={() => onChoose(issue.issueId, selectedChoice.choiceId, confirmingLocalRemoval)}
        >
          {busy && <Loader2 className="animate-spin" aria-hidden="true" />}
          {t(
            confirmingLocalRemoval
              ? 'deviceTrust.actions.confirmExit'
              : 'deviceTrust.actions.confirm'
          )}
        </Button>
      </DialogFooter>
    </>
  )
}
