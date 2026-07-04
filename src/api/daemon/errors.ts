/**
 * Typed error class and error codes for DaemonClient.
 *
 * DaemonClient 的类型化错误类和错误码定义。
 */

/**
 * Error codes mapped from daemon HTTP API responses.
 *
 * 从 daemon HTTP API 响应映射的错误码。
 */
export enum DaemonErrorCode {
  UNAUTHORIZED = 'UNAUTHORIZED',
  FORBIDDEN = 'FORBIDDEN',
  NOT_FOUND = 'NOT_FOUND',
  /**
   * The requested clipboard payload has been demoted to `Lost` (orphaned bytes
   * after capture; cache + spool double miss). Daemon returns 410 Gone.
   * Callers should NOT auto-retry — show the user a clear "content unavailable"
   * message and offer to delete the entry.
   *
   * 剪贴板内容已丢失（capture 后未能落盘到 spool）。Daemon 返回 410 Gone。
   * 前端不应重试，应提示用户内容不可用并允许删除。
   */
  PAYLOAD_UNAVAILABLE = 'PAYLOAD_UNAVAILABLE',
  RATE_LIMITED = 'RATE_LIMITED',
  ENCRYPTION_NOT_READY = 'ENCRYPTION_NOT_READY',
  /**
   * A subsystem is temporarily unavailable (HTTP 503) — e.g. the search index is
   * rebuilding or not yet ready. Distinct from `ENCRYPTION_NOT_READY` (the locked
   * session race) so callers can keep the unlock race quiet while still surfacing
   * real subsystem outages.
   *
   * 某个子系统暂时不可用（HTTP 503），例如搜索索引正在重建或尚未就绪。与
   * `ENCRYPTION_NOT_READY`（会话锁定竞态）区分开，便于调用方在保持解锁竞态
   * 静默的同时，仍能暴露真实的子系统不可用。
   */
  SERVICE_UNAVAILABLE = 'SERVICE_UNAVAILABLE',
  CONFIRMATION_REQUIRED = 'CONFIRMATION_REQUIRED',
  INTERNAL_ERROR = 'INTERNAL_ERROR',
}

/**
 * Structured error from the daemon HTTP API.
 *
 * daemon HTTP API 的结构化错误。
 */
export class DaemonApiError extends Error {
  code: DaemonErrorCode
  details?: unknown

  constructor(code: DaemonErrorCode, message: string, details?: unknown) {
    super(message)
    this.name = 'DaemonApiError'
    this.code = code
    this.details = details
  }
}

/**
 * Map HTTP status code to DaemonErrorCode.
 *
 * 将 HTTP 状态码映射为 DaemonErrorCode。
 */
export function mapStatusToErrorCode(status: number): DaemonErrorCode {
  switch (status) {
    case 401:
      return DaemonErrorCode.UNAUTHORIZED
    case 403:
      return DaemonErrorCode.FORBIDDEN
    case 404:
      return DaemonErrorCode.NOT_FOUND
    case 410:
      return DaemonErrorCode.PAYLOAD_UNAVAILABLE
    case 423:
      // Locked encryption session (`session_locked`). Surfaced as
      // "encryption not ready" so callers gate on a single not-ready condition.
      return DaemonErrorCode.ENCRYPTION_NOT_READY
    case 429:
      return DaemonErrorCode.RATE_LIMITED
    case 503:
      // Subsystem temporarily unavailable (e.g. search index rebuilding). Kept
      // distinct from 423 so a locked-session race stays quiet while a real
      // index outage still surfaces to callers.
      return DaemonErrorCode.SERVICE_UNAVAILABLE
    default:
      return DaemonErrorCode.INTERNAL_ERROR
  }
}
