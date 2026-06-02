/**
 * generated-bridge — wire the @hey-api/openapi-ts generated fetch client to the
 * daemon session lifecycle (ADR-008 P5).
 *
 * 把 @hey-api 生成的 fetch 客户端接入 daemon 的 session 生命周期。
 *
 * What this does (and does NOT do):
 * - Sets the generated client's `baseUrl` from the daemon connection config.
 * - Injects auth as the `?auth=Session <token>` QUERY param via a request
 *   interceptor (the daemon authenticates via query param, NOT a header —
 *   see `client.ts` `sendRequest`/`blobUrl`).
 * - Normalizes the thrown error so a 401 is observable downstream: with
 *   `throwOnError: true` the generated client throws the *parsed error body*
 *   (not an object carrying the Response), so an error interceptor re-wraps it
 *   as a `SdkRequestError` that carries `.response` — which `DaemonClient.callSdk`
 *   reads to drive its one-shot 401 refresh+retry.
 *
 * It does NOT route any existing app code through the SDK — that is P6. This
 * module only makes the typed client AVAILABLE and authenticated.
 *
 * Call `installGeneratedClientBridge(baseUrl)` once, from `DaemonClient.initialize`.
 */

import { client as generatedClient } from '@/api/generated/client.gen'
import { daemonClient } from './client'

/**
 * Error thrown by the generated SDK after the bridge's error interceptor runs.
 *
 * The generated fetch client throws the raw parsed error body on non-2xx; the
 * interceptor wraps it so callers (notably `DaemonClient.callSdk`) can inspect
 * the HTTP status via `.response.status` to detect 401.
 *
 * 生成的 SDK 在非 2xx 时抛出原始错误体；拦截器将其包装为带 `.response` 的错误，
 * 方便 `callSdk` 通过 HTTP 状态码识别 401 并触发刷新重试。
 */
export class SdkRequestError extends Error {
  /** The HTTP response, when one was produced (absent on network errors). */
  response?: Response
  /** The original error payload thrown by the generated client (parsed body or text). */
  cause: unknown

  constructor(cause: unknown, response?: Response) {
    super(
      response
        ? `SDK request failed with status ${response.status}`
        : 'SDK request failed (no response)'
    )
    this.name = 'SdkRequestError'
    this.cause = cause
    this.response = response
  }
}

let installed = false

/**
 * Wire the generated fetch client to the daemon session lifecycle.
 *
 * Idempotent: safe to call again on re-initialize (re-applies baseUrl; the
 * interceptors are registered only once so re-init does not stack duplicates).
 *
 * @param baseUrl Daemon HTTP base URL (e.g. "http://127.0.0.1:PORT").
 */
export function installGeneratedClientBridge(baseUrl: string): void {
  // (a) baseUrl injection — always re-applied so a re-initialize with a new
  //     daemon port takes effect.
  generatedClient.setConfig({ baseUrl })

  if (installed) return
  installed = true

  // (b) Inject auth as ?auth=Session <token> QUERY param (NOT a header).
  //     Request.url is read-only -> rebuild the Request with the rewritten URL.
  //     Re-reads the freshly refreshed token on each call (incl. the retry),
  //     so the 401 refresh+retry in callSdk transparently picks up the new token.
  generatedClient.interceptors.request.use((request: Request) => {
    const token = daemonClient.currentSession?.token
    if (!token) return request
    const url = new URL(request.url)
    url.searchParams.set('auth', `Session ${token}`)
    return new Request(url, request)
  })

  // (c) Wrap thrown errors so the HTTP status is observable. The generated
  //     client throws the parsed error body on non-2xx; without this, callSdk
  //     could not see `response.status === 401`.
  generatedClient.interceptors.error.use((error: unknown, response: Response | undefined) => {
    return new SdkRequestError(error, response)
  })
}
