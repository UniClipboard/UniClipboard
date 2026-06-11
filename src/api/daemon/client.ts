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
import { installGeneratedClientBridge, SdkRequestError } from './generated-bridge'
import type { DaemonConfig, SessionToken } from './types'
import { isSessionExpired } from './types'

const log = createLogger('daemon-client')

/** Session refresh interval: 4 minutes (240 seconds). */
const REFRESH_INTERVAL_MS = 240_000

/**
 * Re-mint the session on resume when it is within this window of expiry.
 *
 * The keep-alive `setInterval` is frozen while the OS suspends the process
 * (sleep / app backgrounded), so a token can silently expire across a sleep.
 * On the next wake (`visibilitychange` / `focus` / `online`) we proactively
 * refresh if the token is gone or about to expire, so the first post-resume
 * request never fires with a stale token (the daemon would reject it with
 * `ExpiredSignature`). See issue #995.
 */
const WAKE_REFRESH_BUFFER_MS = 30_000

/**
 * Max time to wait for the native session IPC before treating the refresh as
 * failed. Without this, a stalled `getDaemonSession()` on resume leaves the
 * pre-emptive/401-retry refresh hanging forever, which freezes the UI on the
 * loading screen until the user restarts the app (issue #995).
 */
const REFRESH_TIMEOUT_MS = 10_000

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
  /** Bound wake handler, registered while initialized (see {@link WAKE_REFRESH_BUFFER_MS}). */
  private wakeHandler: (() => void) | null = null

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
    this.setupWakeListeners()
    // Wire the @hey-api generated fetch client to this session lifecycle
    // (baseUrl + ?auth= query token + 401 observability). Makes the typed SDK
    // AVAILABLE; nothing routes through it yet (ADR-008 P5; consumers in P6).
    installGeneratedClientBridge(config.baseUrl)
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
   * Run a generated (@hey-api) SDK call through the daemon session lifecycle.
   *
   * Mirrors {@link request}: pre-emptive refresh when the session is expired,
   * plus a one-shot refresh + retry on a 401. `installGeneratedClientBridge`
   * configures the generated client to inject the `?auth=` query token and to
   * throw {@link SdkRequestError} (carrying `.response`) on non-2xx, so a 401 is
   * observable here. Call SDK fns with `{ throwOnError: true }` so they resolve
   * to `{ data }` and reject on error.
   *
   * 让生成的 SDK 调用走 daemon 的 session 生命周期：过期预刷新 + 401 单次刷新重试。
   * SDK 函数需以 `{ throwOnError: true }` 调用。错误统一规范化为 {@link DaemonApiError}
   * （与 {@link request} 同形态），下游 wrapper/consumer 的错误分类逻辑无需改动。
   *
   * @param call Thunk invoking a generated SDK fn; resolves to `{ data }`.
   * @returns The unwrapped `data`.
   * @throws {DaemonApiError} If the client is not initialized, or on any HTTP error.
   */
  async callSdk<T>(call: () => Promise<{ data: T }>): Promise<T> {
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

    try {
      const { data } = await call()
      return data
    } catch (err) {
      // Retry once on 401 (session may have been invalidated server-side).
      if (err instanceof SdkRequestError && err.response?.status === 401) {
        this.session = null
        await this.refreshSession()
        try {
          const { data } = await call()
          return data
        } catch (retryErr) {
          throw this.normalizeSdkError(retryErr)
        }
      }
      throw this.normalizeSdkError(err)
    }
  }

  /**
   * Run a generated SDK call against an ENVELOPED endpoint and return the
   * unwrapped payload. The single place that knows the daemon's canonical
   * success shape `ApiEnvelope<T> = { data: T, ts }` (ADR-008 §H): wrappers
   * declare only the payload type instead of unwrapping `envelope.data` by
   * hand. Session lifecycle and error normalization come from {@link callSdk}.
   *
   * 信封端点的唯一拆封入口:连拆 SDK 传输包装与 `{ data, ts }` 信封,
   * wrapper 只声明 payload 类型。`ts` 无消费方,直接丢弃。
   *
   * @param call Thunk invoking a generated SDK fn for an enveloped endpoint.
   * @returns The envelope's `data` payload.
   * @throws {DaemonApiError} Same contract as {@link callSdk}.
   */
  async callEnveloped<T>(call: () => Promise<{ data: { data: T; ts: number } }>): Promise<T> {
    const envelope = await this.callSdk(call)
    return envelope.data
  }

  /**
   * Normalize a thrown SDK error into the same {@link DaemonApiError} shape that
   * {@link request} / {@link handleResponse} produce, so SDK-routed wrappers keep
   * the legacy error contract: `code` from the HTTP status, the full normalized
   * body (`{ code, message, details? }`) on `.details`, and a `"<status> on
   * <path>"` message. This is what existing error-classification consumers rely
   * on — e.g. `ClipboardContent` reads `err.code === PAYLOAD_UNAVAILABLE` (410),
   * `getEntryDetail` swallows `NOT_FOUND` (404), and the setupV2 classifiers
   * regex the status out of `.message` and read the server text from
   * `.details.message`.
   *
   * 把生成 SDK 抛出的 {@link SdkRequestError} 归一为 {@link DaemonApiError}，
   * 与 `request()` 同形态，保持下游错误分类契约不变。非 SDK 错误原样透传。
   *
   * @param err The error caught from a generated SDK call.
   * @returns A {@link DaemonApiError} for SDK HTTP errors; the original error otherwise.
   */
  private normalizeSdkError(err: unknown): unknown {
    if (!(err instanceof SdkRequestError)) {
      return err
    }
    const status = err.response?.status
    const code = status != null ? mapStatusToErrorCode(status) : DaemonErrorCode.INTERNAL_ERROR

    let endpoint = 'sdk request'
    const rawUrl = err.response?.url
    if (rawUrl) {
      try {
        endpoint = new URL(rawUrl).pathname
      } catch {
        endpoint = rawUrl
      }
    }
    const message = status != null ? `${status} on ${endpoint}` : 'SDK request failed'

    // `err.cause` is the parsed normalized body (`{ code, message, details? }`),
    // mirroring `handleResponse`'s `details = body`. Carry it through verbatim so
    // `.details.message` / `.details.details` stay reachable for classifiers.
    return new DaemonApiError(code, message, err.cause)
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
    this.teardownWakeListeners()
    this.session = null
    this.config = null
    this.refreshPromise = null
  }

  // ── Private helpers ──────────────────────────────────────────

  private async doRefreshSession(): Promise<SessionToken> {
    const data = await this.fetchDaemonSession()
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

  /**
   * Ask native Tauri for a session, racing the IPC against a timeout so a
   * stalled native side can't hang the refresh forever. On timeout we reject
   * with a {@link DaemonApiError}; the caller surfaces it (loading-screen
   * watchdog / 401 retry) instead of waiting indefinitely.
   */
  private async fetchDaemonSession(): Promise<
    Awaited<ReturnType<typeof commands.getDaemonSession>>
  > {
    let timer: ReturnType<typeof setTimeout> | undefined
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        reject(
          new DaemonApiError(
            DaemonErrorCode.INTERNAL_ERROR,
            `Daemon session request timed out after ${REFRESH_TIMEOUT_MS}ms`
          )
        )
      }, REFRESH_TIMEOUT_MS)
    })
    try {
      return await Promise.race([commands.getDaemonSession(), timeout])
    } finally {
      if (timer !== undefined) clearTimeout(timer)
    }
  }

  /**
   * Re-mint the session when the app resumes (tab visible / window focused /
   * network back) and the current token is gone or near expiry. The keep-alive
   * timer is frozen while suspended, so this is the path that recovers a token
   * that expired across a sleep before any request fires (issue #995).
   */
  private setupWakeListeners(): void {
    if (typeof window === 'undefined' || this.wakeHandler) return
    const handler = () => {
      // Ignore the "hidden" half of a visibilitychange — only act on resume.
      if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return
      if (!isSessionExpired(this.session, WAKE_REFRESH_BUFFER_MS)) return
      // refreshSession() coalesces concurrent calls, so this is cheap.
      this.refreshSession().catch(err => {
        log.error({ err }, 'wake refresh failed')
      })
    }
    this.wakeHandler = handler
    if (typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', handler)
    }
    window.addEventListener('focus', handler)
    window.addEventListener('online', handler)
  }

  private teardownWakeListeners(): void {
    if (!this.wakeHandler) return
    if (typeof document !== 'undefined') {
      document.removeEventListener('visibilitychange', this.wakeHandler)
    }
    if (typeof window !== 'undefined') {
      window.removeEventListener('focus', this.wakeHandler)
      window.removeEventListener('online', this.wakeHandler)
    }
    this.wakeHandler = null
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
