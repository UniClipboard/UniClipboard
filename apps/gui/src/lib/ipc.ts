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

import * as hostBindings from '@host/hostservice'
import { captureDiagnosticException, recordDiagnosticBreadcrumb } from '@/observability/diagnostics'
import { isExpectedCommandError, toReportableError } from '@/observability/errors'
import { redactSensitiveArgs } from '@/observability/redaction'
import { traceManager } from '@/observability/trace'
import { commands as legacyRaw } from './ipc-bindings.generated'

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
type HostBindings = typeof hostBindings

/** Commands whose contract is the Go service signature (Wails generated bindings). */
type GeneratedCommands = {
  [
    K in keyof HostBindings as K extends 'Invoke' | 'Connection' | 'Session'
      ? never
      : Uncapitalize<K & string>
  ]: HostBindings[K] extends (...args: infer A) => Promise<infer R>
    ? (...args: A) => Promise<R>
    : never
}

type LegacyCommands = Omit<typeof legacyRaw, keyof GeneratedCommands>

export type TypedCommands = {
  [K in keyof LegacyCommands]: Wrap<LegacyCommands[K]>
} & GeneratedCommands
export type { DaemonStartupStatus } from './ipc-bindings.generated'

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

/**
 * A rejection of a generated host binding. Wails rejects with a `RuntimeError` whose `cause` is the payload the
 * host marshalled (`hostapi.Marshal`): the typed error object or string. Anything else (unknown method, wrong
 * argument count, transport failure) has no payload and stays an `Error`, which counts as a system error.
 */
function hostRejection(error: unknown): unknown {
  if (
    error instanceof Error &&
    error.name === 'RuntimeError' &&
    'cause' in error &&
    error.cause != null
  ) {
    return error.cause
  }
  return error
}

/** Breadcrumb, trace, Sentry reporting and error classification shared by every host command. */
async function instrumented<T>(
  name: string,
  args: unknown[],
  call: (trace: TraceArg) => Promise<T>
): Promise<T> {
  const trace = traceManager.startTrace(name)
  const traceArg: TraceArg = {
    trace_id: trace.traceId,
    timestamp: trace.startTime,
  }

  // For Sentry breadcrumbs we redact the *named* arg bag if there's
  // one, otherwise log positional values redacted shallowly. The
  // functions take positional args, so we just attach the
  // tuple — redactSensitiveArgs accepts an object/record only, so
  // wrap the tuple as an object first.
  const safeArgs = name.toLowerCase().includes('visualeffects')
    ? {}
    : redactSensitiveArgs(Object.fromEntries(args.map((value, index) => [`arg${index}`, value])))

  recordDiagnosticBreadcrumb({
    category: 'tauri_command',
    message: name,
    level: 'info',
    data: { traceId: trace.traceId, args: safeArgs },
  })

  try {
    return await call(traceArg)
  } catch (error) {
    // User/validation errors (bad input, wrong passphrase, name taken)
    // are normal product flow handled by the UI — reporting them to
    // Sentry buries real system-error alerts under input-validation
    // noise. Only capture genuinely unexpected failures. The breadcrumb
    // above still records the call for context on later real errors.
    if (!isExpectedCommandError(error)) {
      captureDiagnosticException(toReportableError(error, name), {
        tags: { command: name, traceId: trace.traceId },
        extra: { args: safeArgs },
      })
    }
    throw error
  } finally {
    traceManager.endTrace(trace)
  }
}

const generatedByName: Record<string, (...args: unknown[]) => Promise<unknown>> =
  Object.fromEntries(
    Object.entries(hostBindings)
      .filter(
        ([name, value]) =>
          typeof value === 'function' && !['Invoke', 'Connection', 'Session'].includes(name)
      )
      .map(([name, value]) => [
        name.charAt(0).toLowerCase() + name.slice(1),
        value as (...args: unknown[]) => Promise<unknown>,
      ])
  )

function buildProxy(): TypedCommands {
  return new Proxy(
    {},
    {
      get(_target, prop) {
        if (typeof prop !== 'string') return undefined
        const generated = generatedByName[prop]
        if (generated) {
          return (...args: unknown[]) =>
            instrumented(prop, args, async () => {
              try {
                return await generated(...args)
              } catch (error) {
                throw hostRejection(error)
              }
            })
        }
        const legacy = (legacyRaw as Record<string, unknown>)[prop]
        if (typeof legacy !== 'function') return legacy

        return (...args: unknown[]) =>
          instrumented(prop, args, async traceArg => {
            const result = await (legacy as (...callArgs: unknown[]) => Promise<unknown>)(
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
          })
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
  ConfigCommandError,
  ConfigImportPreview,
  ImportConfigStageResult,
  DaemonBootstrapFailure,
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
