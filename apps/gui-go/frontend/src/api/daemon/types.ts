/**
 * Types for DaemonClient session and connection config.
 *
 * Daemon 客户端连接配置和 session token 类型定义。
 */

/**
 * Daemon connection config received from `get_daemon_connection_info`.
 *
 * 从 `get_daemon_connection_info` 命令接收的 daemon 连接配置。
 */
export interface DaemonConfig {
  /** HTTP base URL, e.g. "http://127.0.0.1:42715" */
  baseUrl: string
  /** WebSocket URL, e.g. "ws://127.0.0.1:42715/ws" */
  wsUrl: string
}

/**
 * JWT session token issued by POST /auth/connect.
 *
 * 由 POST /auth/connect 签发的 JWT session token。
 */
export interface SessionToken {
  /** JWT session token string. */
  token: string
  /** Expiry timestamp in milliseconds (Date.now() + TTL). */
  expiresAt: number
  /** Whether encryption is unlocked on the daemon side. */
  encryptionReady: boolean
}

/**
 * Check whether a session token is expired or absent.
 *
 * 判断 session token 是否过期或不存在。
 *
 * @param session The session to check (null means no session).
 * @param bufferMs Safety buffer in ms before actual expiry (default 5s).
 * @returns true if expired or null.
 */
export function isSessionExpired(session: SessionToken | null, bufferMs = 5000): boolean {
  if (!session) return true
  return Date.now() >= session.expiresAt - bufferMs
}
