import type { TFunction } from 'i18next'
import type { DeviceGroupChoices, DeviceGroupIssue } from '@/api/daemon/device-trust'

export function decisionFingerprint(issue: DeviceGroupIssue): string {
  return JSON.stringify([
    issue.issueId,
    issue.choices.map(choice => [
      choice.choiceId,
      choice.memberDeviceIds,
      choice.membersComplete,
      choice.impact && [
        choice.impact.localDeviceOutcome,
        choice.impact.syncScopeDeviceIds,
        choice.impact.pausedDeviceIds,
        choice.impact.requiresRejoinDeviceIds,
      ],
      choice.requiresRePairing,
    ]),
    issue.reason && [
      issue.reason.kind,
      issue.reason.detailsComplete,
      issue.reason.changes.map(change => [
        change.actor.deviceId,
        change.target.deviceId,
        change.kind,
        change.side,
      ]),
      issue.reason.decisions.map(decision => [
        decision.device.deviceId,
        decision.target.deviceId,
        decision.decision,
      ]),
    ],
  ])
}

export function presentDeviceGroups(groups: DeviceGroupChoices, t: TFunction) {
  const issue = groups.issues[0]
  const localId = groups.deviceTrust.localDeviceId
  const names = new Map<string, string>()
  const aliases = new Map<string, Set<string>>()
  const remember = (id: string, name: string) => {
    names.set(id, name)
    const key = name.trim().toLocaleLowerCase()
    const ids = aliases.get(key) ?? new Set<string>()
    ids.add(id)
    aliases.set(key, ids)
  }
  for (const device of groups.deviceTrust.devices) remember(device.deviceId, device.displayName)
  for (const choice of issue?.choices ?? [])
    for (const member of choice.members ?? []) remember(member.deviceId, member.displayName)
  for (const change of issue?.reason?.changes ?? []) {
    remember(change.actor.deviceId, change.actor.displayName)
    remember(change.target.deviceId, change.target.displayName)
  }
  for (const decision of issue?.reason?.decisions ?? []) {
    remember(decision.device.deviceId, decision.device.displayName)
    remember(decision.target.deviceId, decision.target.displayName)
  }
  const label = (id: string, supplied?: string) => {
    const name = (supplied ?? names.get(id) ?? '').trim()
    const sameName = [...(aliases.get(name.toLocaleLowerCase()) ?? [])]
    const duplicate = sameName.length > 1
    let suffixLength = 4
    while (
      suffixLength < id.length &&
      sameName.some(other => other !== id && other.endsWith(id.slice(-suffixLength)))
    )
      suffixLength += 2
    return !name || duplicate
      ? `${name || t('deviceTrust.presentation.unnamed')} · ${id.slice(-suffixLength).toUpperCase()}`
      : name
  }
  const list = (ids: string[]) =>
    [...new Set(ids)].map(id => label(id)).join(t('deviceTrust.listSeparator'))
  const reason = issue?.reason
  const localRemoval =
    reason?.kind === 'pending_removal'
      ? reason.changes.find(
          change => change.kind === 'removed_device' && change.target.deviceId === localId
        )
      : undefined
  const facts =
    reason?.changes.map(change =>
      t(`deviceTrust.presentation.${change.kind === 'removed_device' ? 'removed' : 'added'}`, {
        actor: label(change.actor.deviceId, change.actor.displayName),
        target: label(change.target.deviceId, change.target.displayName),
      })
    ) ?? []
  const decisions =
    reason?.decisions.map(decision =>
      t(`deviceTrust.presentation.${decision.decision}`, {
        actor: label(decision.device.deviceId, decision.device.displayName),
        target: label(decision.target.deviceId, decision.target.displayName),
      })
    ) ?? []
  return {
    localName: label(localId),
    localRemovalTitle: localRemoval
      ? t('deviceTrust.presentation.localRemovalTitle', {
          actor: label(localRemoval.actor.deviceId, localRemoval.actor.displayName),
        })
      : null,
    reason:
      reason?.kind === 'pending_removal' && facts.length
        ? facts.join(' ')
        : [
            t(`deviceTrust.presentation.reason.${reason?.kind ?? 'unknown'}`),
            ...facts,
            ...decisions,
          ].join(' '),
    detailsIncomplete: reason !== undefined && !reason.detailsComplete,
    choices: (issue?.choices ?? []).map(choice => {
      const members = new Map(choice.members?.map(member => [member.deviceId, member]))
      const memberNames = choice.memberDeviceIds
        .map(id => {
          const name = label(id, members.get(id)?.displayName)
          return id === localId ? t('deviceTrust.presentation.localMember', { name }) : name
        })
        .join(t('deviceTrust.listSeparator'))
      return {
        id: choice.choiceId,
        title: t(
          localRemoval && choice.impact?.localDeviceOutcome === 'removed'
            ? 'deviceTrust.presentation.leave'
            : localRemoval && choice.isCurrentGroup
              ? 'deviceTrust.presentation.keep'
              : choice.choiceId === 'apply' && reason?.kind === 'pending_removal'
                ? 'deviceTrust.modal.applyTitle'
                : choice.isCurrentGroup
                  ? 'deviceTrust.modal.stayTitle'
                  : 'deviceTrust.modal.useGroupTitle'
        ),
        summary:
          localRemoval && choice.impact?.localDeviceOutcome === 'removed'
            ? t('deviceTrust.presentation.leaveEffect')
            : localRemoval && choice.isCurrentGroup && choice.impact?.pausedDeviceIds.length
              ? t('deviceTrust.presentation.keepEffect', {
                  names: list(choice.impact.pausedDeviceIds),
                })
              : choice.impact?.pausedDeviceIds.length
                ? t('deviceTrust.presentation.paused', {
                    names: list(choice.impact.pausedDeviceIds),
                  })
                : '',
        members:
          memberNames ||
          t(
            choice.membersComplete
              ? 'deviceTrust.modal.noDevices'
              : 'deviceTrust.presentation.membersUnknown'
          ),
        membersIncomplete: !choice.membersComplete,
        paused: list(choice.impact?.pausedDeviceIds ?? []),
        pending: list(choice.impact?.pendingConfirmationDeviceIds ?? []),
        scope: list(choice.impact?.syncScopeDeviceIds ?? []),
        rejoin: list(choice.impact?.requiresRejoinDeviceIds ?? []),
        impactKnown: choice.impact != null,
        removesLocal: choice.impact?.localDeviceOutcome === 'removed' || choice.requiresRePairing,
      }
    }),
  }
}
