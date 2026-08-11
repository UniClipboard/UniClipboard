export function isRemovalInProgress(state: {
  phase: string
  removalIntentCount: number
  removed: boolean
}): boolean {
  return !state.removed && state.removalIntentCount > 0 && state.phase !== 'complete'
}

export function isWaitingForDeviceUpdate(
  state: { phase: string; waitingMemberDeviceIds: string[] },
  deviceId: string
): boolean {
  return (
    state.phase === 'waiting_for_offline_member' && state.waitingMemberDeviceIds.includes(deviceId)
  )
}
