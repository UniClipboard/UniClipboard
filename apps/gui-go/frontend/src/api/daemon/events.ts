/**
 * Shared event utilities for daemon WebSocket event handling.
 *
 * Provides classifyPairingError: maps error strings to PairingErrorKind.
 */

export type PairingErrorKind =
  | 'active_session_exists'
  | 'no_local_participant'
  | 'session_not_found'
  | 'daemon_unavailable'
  | 'unknown'

/**
 * Maps a raw error string to a typed PairingErrorKind.
 *
 * 将原始错误字符串映射到类型化的 PairingErrorKind。
 */
export function classifyPairingError(rawError?: string | null): PairingErrorKind {
  const normalized = rawError?.toLowerCase() ?? ''

  if (
    normalized.includes('active pairing session exists') ||
    normalized.includes('active_session_exists')
  ) {
    return 'active_session_exists'
  }

  if (
    normalized.includes('no local pairing participant ready') ||
    normalized.includes('no_local_participant')
  ) {
    return 'no_local_participant'
  }

  if (
    normalized.includes('pairing session not found') ||
    normalized.includes('session_not_found') ||
    normalized.includes('session expired')
  ) {
    return 'session_not_found'
  }

  if (
    normalized.includes('daemon connection info is not available') ||
    normalized.includes('connection refused') ||
    normalized.includes('failed to call daemon pairing route') ||
    normalized.includes('failed to open daemon tcp socket') ||
    normalized.includes('failed to connect daemon websocket') ||
    normalized.includes('pairing_host_unavailable')
  ) {
    return 'daemon_unavailable'
  }

  return 'unknown'
}
