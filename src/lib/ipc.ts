/**
 * Typed IPC entry point — wraps the auto-generated `commands` from
 * `ipc-bindings.generated.ts` with our existing observability stack
 * (`invokeWithTrace`-style trace_id injection + Sentry breadcrumb +
 * arg redaction) without losing the typed signatures.
 *
 * ## Why this layer exists
 *
 * 1. `tauri-specta` codegen emits `__TAURI_INVOKE` calls hard-coded to the
 *    `@tauri-apps/api/core` import. There's no hook to swap that for our
 *    `invokeWithTrace`. So we wrap the *generated* `commands` here, calling
 *    each generated method with our trace metadata appended as the last
 *    positional argument and unwrapping the `{status, data|error}` typed
 *    result back to throw-on-error semantics — matching what the rest of
 *    the codebase expects.
 *
 * 2. Type safety: Rust signatures change → `cargo test --test specta_export`
 *    rewrites `ipc-bindings.generated.ts` → `commands.xxx` here picks up new
 *    arg/return types → call sites that didn't update fail `tsc`.
 *
 * 3. Trace correlation that *actually works*: the legacy `invokeWithTrace`
 *    sends a wire field named `_trace`, but Tauri's `#[command]` macro
 *    strips the leading underscore and exposes the param as the wire field
 *    `trace` — so the legacy path silently drops trace metadata. The
 *    generated bindings here use `trace`, so trace_id finally lands on the
 *    Rust span fields where `record_trace_fields` was waiting.
 *
 * ## Migration notes
 *
 * Call sites should switch from
 *   `await invokeWithTrace<T>('cmd_name', { ... })`
 * to
 *   `await commands.cmdName({ ... })` (named-args sugar — see below)
 * or
 *   `await commands.cmdName(arg1, arg2)` (positional, mirrors the generated signature).
 *
 * The wrapper transparently injects trace + redacts logs + bubbles errors.
 */

import { isExpectedCommandError, toReportableError } from '@/observability/errors'
import { redactSensitiveArgs } from '@/observability/redaction'
import { Sentry } from '@/observability/sentry'
import { traceManager } from '@/observability/trace'
import { commands as raw } from './ipc-bindings.generated'

/** Wire shape of the trace metadata Tauri commands accept. */
type TraceArg = { trace_id: string; timestamp: number } | null

/**
 * If `Args` ends with the trace tuple element (`TraceArg`), drop it.
 * Otherwise leave it alone — some commands (e.g. `getTauriPid`,
 * macOS-only window plugins) don't accept trace.
 */
type StripTrailingTrace<Args extends unknown[]> = Args extends [...infer Init, TraceArg]
  ? Init
  : Args

/**
 * Unwrap the `{status: "ok", data: T} | {status: "error", error: E}` envelope
 * that tauri-specta wraps typed-error commands in. The union is collapsed by
 * `UnwrapInner` (which distributes over the union members):
 *
 * - `{status: "ok", data: D}` → `D` (the resolved value)
 * - `{status: "error", error: E}` → `never` (we rethrow, so it's not returned)
 * - otherwise → the raw value (commands without typed errors are unchanged)
 *
 * The `never` collapses out of the resulting union, so the caller sees
 * exactly the success type — no leaked envelope shape in TS hovers / autocomplete.
 */
type UnwrapInner<T> = T extends { status: 'ok'; data: infer D }
  ? D
  : T extends { status: 'error' }
    ? never
    : T

type UnwrapResult<R> = R extends Promise<infer Inner> ? Promise<UnwrapInner<Inner>> : R

type Wrap<F> = F extends (...args: infer A) => infer R
  ? (...args: StripTrailingTrace<A>) => UnwrapResult<R>
  : F

/**
 * The proxied `commands` object. Same keys as the generated `raw`, but
 * each method drops the trailing `trace` arg and rejects with the typed
 * error directly instead of returning a discriminated union.
 */
export type TypedCommands = { [K in keyof typeof raw]: Wrap<(typeof raw)[K]> }

/**
 * Inspect a result envelope to decide whether tauri-specta wrapped it for
 * a typed error. Commands that return plain values come through unchanged.
 */
function isTypedErrorEnvelope(
  value: unknown
): value is { status: 'ok' | 'error'; data?: unknown; error?: unknown } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'status' in value &&
    typeof (value as { status: unknown }).status === 'string'
  )
}

function buildProxy(): TypedCommands {
  return new Proxy(
    {},
    {
      get(_target, prop) {
        if (typeof prop !== 'string') return undefined
        const generated = (raw as Record<string, unknown>)[prop]
        if (typeof generated !== 'function') return generated

        return async (...args: unknown[]) => {
          const trace = traceManager.startTrace(prop)
          const traceArg: TraceArg = {
            trace_id: trace.traceId,
            timestamp: trace.startTime,
          }

          // For Sentry breadcrumbs we redact the *named* arg bag if there's
          // one, otherwise log positional values redacted shallowly. The
          // generated functions take positional args, so we just attach the
          // tuple — redactSensitiveArgs accepts an object/record only, so
          // wrap the tuple as an object first.
          const safeArgs = redactSensitiveArgs(
            Object.fromEntries(args.map((value, index) => [`arg${index}`, value]))
          )

          Sentry.addBreadcrumb({
            category: 'tauri_command',
            message: prop,
            level: 'info',
            data: { traceId: trace.traceId, args: safeArgs },
          })

          try {
            const result = await (generated as (...callArgs: unknown[]) => Promise<unknown>)(
              ...args,
              traceArg
            )

            if (isTypedErrorEnvelope(result)) {
              if (result.status === 'ok') return result.data
              // typed error: rethrow as-is so call sites can pattern-match on
              // the Rust-side discriminated union (e.g. `error.code`).
              throw result.error
            }
            return result
          } catch (error) {
            // User/validation errors (bad input, wrong passphrase, name taken)
            // are normal product flow handled by the UI — reporting them to
            // Sentry buries real system-error alerts under input-validation
            // noise. Only capture genuinely unexpected failures. The breadcrumb
            // above still records the call for context on later real errors.
            if (!isExpectedCommandError(error)) {
              Sentry.captureException(toReportableError(error, prop), {
                tags: { command: prop, traceId: trace.traceId },
                extra: { args: safeArgs },
              })
            }
            throw error
          } finally {
            traceManager.endTrace()
          }
        }
      },
    }
  ) as TypedCommands
}

/**
 * Typed Tauri command client. Prefer this over the legacy
 * `invokeWithTrace('cmd_name', args)` — Rust signature changes propagate to
 * compile errors instead of runtime serde failures.
 *
 * @example
 * ```ts
 * const meta = await commands.getDeviceMeta()
 * await commands.setTrayLanguage('en')
 * try {
 *   const result = await commands.unlockSpaceWithPassphrase({ passphrase: 'hunter2' })
 *   console.log(result.spaceId)
 * } catch (error) {
 *   if (typeof error === 'object' && error && 'code' in error) {
 *     // typed UnlockSpaceCommandError
 *   }
 * }
 * ```
 */
export const commands: TypedCommands = buildProxy()

// ADR-008 P3-3 (B2'-3): no tauri-specta events. The former
// `clipboardDeliveryStatusChanged` Tauri event was retired once the GUI became
// a pure client — delivery refetch signals now travel over the daemon WS
// (`clipboard.delivery_status_changed`, GAP-WS-1), consumed via
// `daemonWs.subscribe(['clipboard'])` in `useEntryDelivery`.

// Re-export the generated DTO/error types so call sites can `import { type
// CommandError } from '@/lib/ipc'` without having to know about the generated
// file path. Keeps the generated artifact a hidden implementation detail.
// (Mobile-sync types moved to `@/api/tauri-command/mobile_sync` in ADR-008
// P3-b when those commands became daemon HTTP endpoints.)
export type {
  CommandError,
  DaemonConnectionPayload,
  DeviceMeta,
  DownloadEvent,
  DownloadPhase,
  DownloadProgressSnapshot,
  InstallKind,
  ShortcutKeyDto,
  TraceMetadata,
  UpdateKeyboardShortcutsResult,
  UpdateMetadata,
} from './ipc-bindings.generated'
