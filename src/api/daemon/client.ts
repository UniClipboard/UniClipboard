/**
 * DaemonClient — singleton HTTP client for the local daemon API.
 *
 * Daemon 单例 HTTP 客户端，管理 session token 生命周期并提供类型化请求方法。
 *
 * # Bootstrap Flow / 启动流程
 * 1. Frontend polls `get_daemon_connection_info` until it receives `{ baseUrl, wsUrl }`.
 * 2. Call `daemonClient.initialize(config)` with the received config.
 * 3. The client asks native Tauri for short-lived daemon sessions every 4 minutes.
 *
 * # Session Token Lifecycle / Session Token 生命周期
 * - Native Tauri exchanges bearer token → JWT session token (TTL 300s, refresh at 240s).
 * - `request<T>()` auto-refreshes on 401 (one retry).
 * - `destroy()` clears the keep-alive timer.
 */

import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { DaemonApiError, DaemonErrorCode, mapStatusToErrorCode } from './errors'
import type { DaemonConfig, SessionToken } from './types'
import { isSessionExpired } from './types'

const log = createLogger('daemon-client')

/** Session refresh interval: 4 minutes (240 seconds). */
const REFRESH_INTERVAL_MS = 240_000

/**
 * Options for `request<T>()`.
 *
 * 请求选项，允许调用方自定义 HTTP 方法、请求体和请求头。
 */
export interface RequestOptions {
  method?: string
  body?: unknown
  headers?: Record<string, string>
  /** Skip automatic session refresh on 401. */
  skipRetry?: boolean
  /** AbortSignal for request cancellation. */
  signal?: AbortSignal
}

class DaemonClient {
  private config: DaemonConfig | null = null
  private session: SessionToken | null = null
  private refreshTimer: ReturnType<typeof setInterval> | null = null
  private refreshPromise: Promise<SessionToken> | null = null

  /**
   * Whether the client has been initialized with daemon connection config.
   *
   * 客户端是否已用 daemon 连接配置初始化。
   */
  get initialized(): boolean {
    return this.config !== null
  }

  /**
   * The WebSocket URL from the daemon connection config, if available.
   *
   * daemon 配置中的 WebSocket URL（如可用）。
   */
  get wsUrl(): string | null {
    return this.config?.wsUrl ?? null
  }

  /**
   * The current session token object, if available.
   *
   * 当前 session token 对象（如可用）。
   */
  get currentSession(): SessionToken | null {
    return this.session
  }

  /**
   * Initialize with daemon connection config.
   * Starts the session keep-alive timer.
   *
   * 用 daemon 连接配置初始化，并启动 session 保活定时器。
   *
   * @param config Connection config returned by `get_daemon_connection_info`.
   */
  initialize(config: DaemonConfig): void {
    this.config = config
    this.startKeepAlive()
  }

  /**
   * Ask native Tauri for a short-lived daemon session token.
   *
   * 向 Tauri 原生侧请求短期 daemon session token。
   *
   * @returns The new session token.
   * @throws {DaemonApiError} If the request fails.
   */
  async refreshSession(): Promise<SessionToken> {
    if (!this.config) {
      throw new DaemonApiError(
        DaemonErrorCode.INTERNAL_ERROR,
        'DaemonClient not initialized — call initialize() first'
      )
    }

    // Coalesce concurrent refresh calls to avoid duplicate POST /auth/connect.
    if (this.refreshPromise) {
      return this.refreshPromise
    }

    this.refreshPromise = this.doRefreshSession()
    try {
      const result = await this.refreshPromise
      return result
    } finally {
      this.refreshPromise = null
    }
  }

  /**
   * Send a typed HTTP request through the daemon API.
   * Auto-refreshes the session token if expired or on 401.
   *
   * 通过 daemon API 发送类型化 HTTP 请求。session 过期或 401 时自动刷新。
   *
   * @param endpoint API path (e.g. "/settings").
   * @param options Request options.
   * @returns Parsed response body.
   * @throws {DaemonApiError} On HTTP errors.
   */
  async request<T>(endpoint: string, options: RequestOptions = {}): Promise<T> {
    if (!this.config) {
      throw new DaemonApiError(
        DaemonErrorCode.INTERNAL_ERROR,
        'DaemonClient not initialized — call initialize() first'
      )
    }

    // Pre-emptive refresh if session is expired.
    if (isSessionExpired(this.session)) {
      await this.refreshSession()
    }

    const response = await this.sendRequest(endpoint, options)

    // Auto-retry once on 401 (session may have been invalidated server-side).
    if (response.status === 401 && !options.skipRetry) {
      this.session = null
      await this.refreshSession()
      const retryResponse = await this.sendRequest(endpoint, { ...options, skipRetry: true })
      return this.handleResponse<T>(retryResponse, endpoint)
    }

    return this.handleResponse<T>(response, endpoint)
  }

  /**
   * Build a full daemon URL for binary resource access with session auth.
   * Suitable for use in <img src> without JavaScript fetch.
   *
   * 构建带 session 认证的 daemon 二进制资源完整 URL。
   * 适用于 <img src> 等无法设置请求头的场景。
   *
   * @param path API path (e.g. "/clipboard/blobs/abc-123").
   * @returns Full URL string with ?auth= query param, or null if client not ready.
   */
  blobUrl(path: string): string | null {
    if (!this.config || !this.session?.token) return null
    if (
      path.startsWith('data:') ||
      path.startsWith('blob:') ||
      path.startsWith('http://') ||
      path.startsWith('https://')
    ) {
      return path
    }
    const url = new URL(`${this.config.baseUrl}${path}`)
    url.searchParams.set('auth', `Session ${this.session.token}`)
    return url.toString()
  }

  /**
   * Stop the keep-alive timer and clear session state.
   *
   * 停止保活定时器并清除 session 状态。
   */
  destroy(): void {
    this.stopKeepAlive()
    this.session = null
    this.config = null
    this.refreshPromise = null
  }

  // ── Private helpers ──────────────────────────────────────────

  private async doRefreshSession(): Promise<SessionToken> {
    const data = await commands.getDaemonSession()
    if (!data) {
      throw new DaemonApiError(DaemonErrorCode.UNAUTHORIZED, 'Daemon session is not available yet')
    }

    const now = Date.now()
    this.session = {
      token: data.sessionToken,
      expiresAt: now + data.expiresInSecs * 1000,
      encryptionReady: false, // Phase 75: always false; updated by separate encryption state check
    }

    return this.session
  }

  private async sendRequest(endpoint: string, options: RequestOptions): Promise<Response> {
    const config = this.config!
    const url = new URL(`${config.baseUrl}${endpoint}`)
    const headers: Record<string, string> = { ...options.headers }
    const hasBody = options.body !== undefined

    if (this.session?.token) {
      url.searchParams.set('auth', `Session ${this.session.token}`)
    }

    if (hasBody) {
      headers['Content-Type'] = 'application/json'
    }

    return fetch(url, {
      method: options.method ?? 'GET',
      headers,
      body: hasBody ? JSON.stringify(options.body) : undefined,
      signal: options.signal,
    })
  }

  private async handleResponse<T>(response: Response, endpoint: string): Promise<T> {
    if (!response.ok) {
      const errorCode = mapStatusToErrorCode(response.status)
      let message = `${response.status} on ${endpoint}`
      let details: unknown
      try {
        const body = await response.json()
        if (body.error) message = body.error
        details = body
      } catch {
        // ignore parse failures
      }
      throw new DaemonApiError(errorCode, message, details)
    }

    // Treat empty success responses as void even when the server uses 200 OK.
    if (response.status === 204 || response.status === 205) {
      return undefined as T
    }

    const body = await response.text()
    if (body.trim().length === 0) {
      return undefined as T
    }

    return JSON.parse(body) as T
  }

  private startKeepAlive(): void {
    this.stopKeepAlive()
    this.refreshTimer = setInterval(() => {
      this.refreshSession().catch(err => {
        log.error({ err }, 'keep-alive refresh failed')
      })
    }, REFRESH_INTERVAL_MS)
  }

  private stopKeepAlive(): void {
    if (this.refreshTimer !== null) {
      clearInterval(this.refreshTimer)
      this.refreshTimer = null
    }
  }
}

/** Singleton DaemonClient instance. */
export const daemonClient = new DaemonClient()
