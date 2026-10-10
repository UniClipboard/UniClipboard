/**
 * Typed entry point to the desktop host commands.
 *
 * The contract is the Go service: every exported method of `HostService` (`apps/gui-go`) is a command, and the
 * Wails generator derives the TypeScript calls and models from the Go signatures into `@host/hostservice` and
 * `@host/models` (`bun run gen:host-contract`). This file does not restate any signature: `commands` is the
 * generated module re-keyed in camelCase, each call wrapped with what every command shares.
 *
 * ## What the wrapper adds
 *
 * 1. Observability: a trace, a Sentry breadcrumb with redacted arguments, and Sentry capture of failures.
 * 2. Error shape: Wails rejects a failed call with a `RuntimeError` whose `cause` is the payload the host marshalled
 *    (`internal/hostapi`): the `{ code, message }` object, the `{ kind }` config union, or a plain string. The
 *    wrapper rethrows that payload, so call sites pattern-match on `error.code` / `error.kind`. Anything without a
 *    payload (wrong argument count, transport failure, a panic) stays an `Error`.
 * 3. Severity: a rejection is a normal product outcome only when the Go error catalog says so
 *    (`isExpectedCommandError`); everything else is reported to Sentry.
 *
 * Which codes a command can reject with is not part of the generated signature (a Go method only returns `error`);
 * see `host-errors.generated.ts` and docs/architecture/gui-go-host-commands.md.
 */

import * as hostBindings from '@host/hostservice'
import type { UpdateKeyboardShortcutsResult } from '@host/models'
import type { ProfileRecoveryResponse, ShortcutKeyDto } from '@/api/generated/types.gen'
import { captureDiagnosticException, recordDiagnosticBreadcrumb } from '@/observability/diagnostics'
import { isExpectedCommandError, toReportableError } from '@/observability/errors'
import { redactSensitiveArgs } from '@/observability/redaction'
import { traceManager } from '@/observability/trace'
import type { DaemonStartupStatus } from './daemon-startup-types'

type HostBindings = typeof hostBindings

/** Every command: the generated function, re-keyed in camelCase and resolving to its result. */
type GeneratedCommands = {
  [K in keyof HostBindings as Uncapitalize<K & string>]: HostBindings[K] extends (
    ...args: infer A
  ) => Promise<infer R>
    ? (...args: A) => Promise<R>
    : never
}

/**
 * Commands whose payload belongs to the daemon, not to the host. Wails generates `any` for opaque JSON (Go
 * `json.RawMessage`), so the daemon-owned type is applied here and nowhere else: the OpenAPI client owns
 * `ProfileRecoveryResponse` and `ShortcutKeyDto`; the startup route is outside OpenAPI (see daemon-startup-types.ts).
 */
type DaemonOwnedCommands = {
  getDaemonStartupStatus: () => Promise<DaemonStartupStatus | null>
  getProfileRecovery: () => Promise<ProfileRecoveryResponse>
  updateKeyboardShortcuts: (shortcuts: Record<string, ShortcutKeyDto | null>) => Promise<
    Omit<UpdateKeyboardShortcutsResult, 'keyboardShortcuts'> & {
      keyboardShortcuts: Record<string, ShortcutKeyDto>
    }
  >
}

export type TypedCommands = Omit<GeneratedCommands, keyof DaemonOwnedCommands> & DaemonOwnedCommands

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
async function instrumented<T>(name: string, args: unknown[], call: () => Promise<T>): Promise<T> {
  const trace = traceManager.startTrace(name)

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
    return await call()
  } catch (rejection) {
    const error = hostRejection(rejection)
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

const uncapitalize = (name: string): string => name.charAt(0).toLowerCase() + name.slice(1)

function buildCommands(): TypedCommands {
  const entries = Object.entries(hostBindings)
    .filter(([, value]) => typeof value === 'function')
    .map(([exported, generated]) => {
      const name = uncapitalize(exported)
      const call = generated as (...args: unknown[]) => Promise<unknown>
      return [name, (...args: unknown[]) => instrumented(name, args, () => call(...args))]
    })
  return Object.fromEntries(entries) as TypedCommands
}

/**
 * The host commands. A Go signature change fails `tsc` at the call sites that did not follow it.
 *
 * @example
 * ```ts
 * const meta = await commands.getDeviceMeta()
 * await commands.setTrayLanguage('en')
 * try {
 *   await commands.unlockContent({ passphrase })
 * } catch (error) {
 *   const { code } = error as ContentUnlockError // a ContentUnlockErrorCode
 * }
 * ```
 */
export const commands: TypedCommands = buildCommands()

// Re-export the generated models and error types so call sites can `import { type DeviceMeta } from '@/lib/ipc'`
// without knowing where the generated files live. The Go enums are TypeScript enums: their members are runtime
// values (`InstallKind.InstallKindDeb`), so they are exported as values.
export type { DaemonStartupStatus } from './daemon-startup-types'
export type {
  CommandError,
  ConfigCommandError,
  ContentUnlockError,
  TextCommandError,
} from './host-errors.generated'
export type {
  ConfigImportPreview,
  DaemonBootstrapFailure,
  DaemonConnection,
  DaemonSession,
  DesktopTheme,
  DesktopThemeSnapshot,
  DeviceMeta,
  DownloadEvent,
  DownloadEventData,
  DownloadProgressSnapshot,
  EffectsSample,
  EffectsSnapshot,
  ImportConfigStageResult,
  SamplePermit,
  UpdateKeyboardShortcutsResult,
  UpdateMetadata,
} from '@host/models'
export {
  DownloadEventKind,
  DownloadPhase,
  EffectsMode,
  InstallKind,
  ModifierDoubleTapAvailability,
  QuickPanelDoubleTapModifier,
  QuickPanelExpandSide,
  QuickPanelPosition,
  SystemMotion,
} from '@host/models'
