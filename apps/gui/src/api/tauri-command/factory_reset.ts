/**
 * Factory-reset error taxonomy (FE-native).
 *
 * ADR-008 P3-1 / D15: factory-reset moved onto the daemon loopback API (see
 * `@/api/daemon/encryption` `factoryResetSpace` + `@/api/security` `resetSpace`).
 * The in-process Tauri command was deleted, so this module no longer wraps a
 * command — it owns the typed error union that `security.ts` translates
 * `DaemonApiError`s into and that `UnlockPage` switches on by `code`.
 *
 * Mirrors the daemon `factory_reset` error surface
 * (`uc-application::facade::space_setup::FactoryResetError`); `FACADE_UNAVAILABLE`
 * is the client-side bucket for "503 runtime_unavailable".
 */

/** Typed error union for factory reset. Switch on `code`. */
export type FactoryResetError =
  | { code: 'FACADE_UNAVAILABLE' }
  | { code: 'KEY_MATERIAL_WIPE_FAILED'; message: string }
  | { code: 'STORAGE_FAILED'; message: string }
  | { code: 'INTERNAL'; message: string }

/** Type guard for `resetSpace` rejections. */
export function isFactoryResetError(error: unknown): error is FactoryResetError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string'
  )
}
