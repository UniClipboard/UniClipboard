import type { ContentUnlockError } from './ipc-bindings.generated'

const errorKeys: Record<ContentUnlockError['code'], string> = {
  WRONG_PASSPHRASE: 'unlock.errors.wrongPassphrase',
  CORRUPTED_KEY_MATERIAL: 'profileRecovery.corrupt',
  SETUP_NOT_COMPLETED: 'unlock.errors.setupNotCompleted',
  SPACE_NOT_INITIALIZED: 'unlock.errors.spaceNotInitialized',
  PROFILE_RECOVERY_PARTIAL: 'profileRecovery.partial',
  PROFILE_RECOVERY_UNSUPPORTED: 'profileRecovery.unsupported',
  PROFILE_RECOVERY_PERSISTENCE_FAILED: 'profileRecovery.persistenceFailed',
  PROFILE_RECOVERY_REQUIRED: 'profileRecovery.description',
  INTERNAL: 'unlock.errors.internal',
}

/** Never display raw IPC/server messages, including errors outside the typed contract. */
export function contentUnlockErrorKey(error: unknown): string {
  const code =
    typeof error === 'object' && error !== null && 'code' in error ? String(error.code) : ''
  return Object.prototype.hasOwnProperty.call(errorKeys, code)
    ? errorKeys[code as ContentUnlockError['code']]
    : 'unlock.errors.internal'
}
