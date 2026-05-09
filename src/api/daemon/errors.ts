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
    case 429:
      return DaemonErrorCode.RATE_LIMITED
    case 503:
      return DaemonErrorCode.ENCRYPTION_NOT_READY
    default:
      return DaemonErrorCode.INTERNAL_ERROR
  }
}
