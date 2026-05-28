import type { useTranslation } from 'react-i18next'
import { isMobileSyncError, type MobileSyncError } from '@/api/tauri-command/mobile_sync'

/**
 * Translate Tauri-emitted mobile-sync errors into user-facing i18n strings.
 * Covers the settings/restart path; register-only variants fall through to `unknown`.
 */
export function translateMobileSyncError(
  t: ReturnType<typeof useTranslation>['t'],
  err: unknown
): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileSync.errors.facadeUnavailable')
      case 'INVALID_LAN_PARAMETER':
        return t('devices.mobileSync.errors.invalidLanParameter', { reason: e.reason })
      case 'SETTINGS_LOAD_FAILED':
        return t('devices.mobileSync.errors.settingsLoadFailed', { message: e.message })
      case 'SETTINGS_SAVE_FAILED':
        return t('devices.mobileSync.errors.settingsSaveFailed', { message: e.message })
      case 'ENDPOINT_INFO_FAILED':
        return t('devices.mobileSync.errors.endpointInfoFailed', { message: e.message })
      case 'LAN_PROBE_FAILED':
        return t('devices.mobileSync.errors.lanProbeFailed', { message: e.message })
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileSync.errors.persistenceFailed', { message: e.message })
      default: {
        const message = (e as { message?: string }).message ?? e.code
        return t('devices.mobileSync.errors.unknown', { message })
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileSync.errors.unknown', { message })
}
