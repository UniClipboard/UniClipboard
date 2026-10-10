/**
 * Space-setup unlock error taxonomy (FE-native).
 *
 * ADR-008 P3-1 / D15: passphrase + silent unlock moved onto the daemon loopback
 * API (see `@/api/daemon/encryption` + `@/api/security`). The in-process Tauri
 * commands were deleted, so this module no longer wraps any command — it just
 * owns the typed error union that `security.ts` translates `DaemonApiError`s
 * into and that `UnlockPage` switches on by `code`.
 *
 * The union mirrors the daemon `unlock_space` error surface
 * (`uc-application::facade::space_setup::UnlockSpaceError`); `FACADE_UNAVAILABLE`
 * is the client-side bucket for "503 runtime_unavailable" (daemon facade not yet
 * assembled).
 */

/** Typed error union for passphrase unlock. Switch on `code`. */
export type UnlockSpaceError =
  | { code: 'FACADE_UNAVAILABLE' }
  | { code: 'SETUP_NOT_COMPLETED' }
  | { code: 'SPACE_NOT_INITIALIZED' }
  | { code: 'WRONG_PASSPHRASE' }
  | { code: 'CORRUPTED_KEY_MATERIAL' }
  | { code: 'INTERNAL'; message: string }

/** Type guard for `unlockSpaceWithPassphrase` rejections. */
export function isUnlockSpaceError(error: unknown): error is UnlockSpaceError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string'
  )
}
