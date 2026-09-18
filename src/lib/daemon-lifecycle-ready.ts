export type EncryptionStatusView = {
  initialized: boolean
  session_ready: boolean
}

export function shouldSignalDaemonLifecycleReady(
  isSetupActive: boolean,
  daemonBootstrapReady: boolean,
  encryptionStatus: EncryptionStatusView | null,
  spaceReadiness: 'checking' | 'recoveringMembership' | 'ready'
): boolean {
  return (
    !isSetupActive &&
    daemonBootstrapReady &&
    encryptionStatus?.initialized === true &&
    encryptionStatus.session_ready === true &&
    spaceReadiness === 'ready'
  )
}
