import { useState } from 'react'
import { commands } from '@/lib/ipc'
import type { ConfigCommandError, ConfigImportPreview, ImportConfigStageResult } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('config-import')

/**
 * Import flow state machine, shared by the in-app settings entry
 * (`ConfigBackupGroup`) and the first-run setup entry (`ImportConfigScreen`):
 * - `idle`       — no bundle picked yet
 * - `password`   — a file was picked; collecting the import passphrase
 * - `confirm`    — manifest previewed; showing the device-move confirmation
 * - `restarting` — forced terminal state while the daemon + GUI restart to
 *   apply the staged migration on the next boot
 */
export type ConfigImportPhase = 'idle' | 'password' | 'confirm' | 'restarting'

/**
 * Classified, presentation-agnostic import failure. Each surface maps these to
 * its own copy (toast vs. inline) so the command sequence and daemon error-code
 * classification stay single-sourced here.
 */
export type ConfigImportErrorKind = 'invalidPassword' | 'incompatible' | 'generic'

interface UseConfigImportOptions {
  /** Reports a classified failure. Cancellations are swallowed silently. */
  onError?: (kind: ConfigImportErrorKind, error: unknown) => void
  /** Fires once the bundle is staged, right before the restart kicks in. */
  onStaged?: (result: ImportConfigStageResult) => void
}

/**
 * Narrow an unknown thrown value to the typed {@link ConfigCommandError}. The
 * `commands` proxy rethrows the Rust-side discriminated union verbatim, so call
 * sites can branch on `kind` / daemon `code` instead of scraping messages.
 */
export function asConfigError(error: unknown): ConfigCommandError | null {
  if (typeof error === 'object' && error !== null && 'kind' in error) {
    return error as ConfigCommandError
  }
  return null
}

export function isCancelled(error: unknown): boolean {
  return asConfigError(error)?.kind === 'cancelled'
}

/** Map a daemon error `code` to a presentation-agnostic {@link ConfigImportErrorKind}. */
function classify(error: unknown): ConfigImportErrorKind {
  const cfg = asConfigError(error)
  if (cfg?.kind === 'daemon') {
    if (cfg.code === 'INVALID_PASSWORD_OR_CORRUPT') return 'invalidPassword'
    if (cfg.code === 'INCOMPATIBLE_BUNDLE') return 'incompatible'
  }
  return 'generic'
}

export interface UseConfigImport {
  phase: ConfigImportPhase
  sourcePath: string | null
  password: string
  setPassword: (value: string) => void
  preview: ConfigImportPreview | null
  stagedResult: ImportConfigStageResult | null
  busy: boolean
  isRestarting: boolean
  /** Pop the native open dialog; advances to `password` once a file is chosen. */
  pickFile: () => Promise<void>
  /** Decrypt the manifest and advance to `confirm`. */
  submitPassword: () => Promise<void>
  /** Stage the bundle and drive the daemon + GUI restart. */
  confirmImport: () => Promise<void>
  /** Step back from `confirm` to `password`, keeping the chosen file. */
  back: () => void
  /** Reset to `idle` (no-op while restarting). */
  reset: () => void
}

/**
 * Headless driver for the config-import flow. Owns the phase machine and the
 * `pick → preview → stage → restart` command sequence; the consuming component
 * renders whatever UI it wants on top (a settings dialog or a full-screen setup
 * step) and decides how failures surface via the `onError` callback.
 */
export function useConfigImport(options: UseConfigImportOptions = {}): UseConfigImport {
  const { onError, onStaged } = options
  const [phase, setPhase] = useState<ConfigImportPhase>('idle')
  const [sourcePath, setSourcePath] = useState<string | null>(null)
  const [password, setPassword] = useState('')
  const [preview, setPreview] = useState<ConfigImportPreview | null>(null)
  const [stagedResult, setStagedResult] = useState<ImportConfigStageResult | null>(null)
  const [busy, setBusy] = useState(false)

  const isRestarting = phase === 'restarting'

  const pickFile = async () => {
    try {
      const path = await commands.pickConfigBundlePath()
      // Cancelled the open dialog — silent.
      if (path === null) return
      setSourcePath(path)
      setPassword('')
      setPreview(null)
      setPhase('password')
    } catch (error) {
      if (isCancelled(error)) return
      log.error({ err: error }, 'Failed to pick config bundle')
      onError?.('generic', error)
    }
  }

  const submitPassword = async () => {
    if (!sourcePath) return
    setBusy(true)
    try {
      const next = await commands.previewConfigImport(password, sourcePath)
      setPreview(next)
      setPhase('confirm')
    } catch (error) {
      if (isCancelled(error)) return
      log.error({ err: error }, 'Failed to preview config import')
      onError?.(classify(error), error)
    } finally {
      setBusy(false)
    }
  }

  const confirmImport = async () => {
    if (!sourcePath) return
    setBusy(true)
    let result: ImportConfigStageResult
    try {
      result = await commands.importConfigPackage(password, sourcePath)
    } catch (error) {
      setBusy(false)
      if (isCancelled(error)) {
        setPhase('confirm')
        return
      }
      log.error({ err: error }, 'Failed to import config')
      onError?.(classify(error), error)
      // Staging failed — drop back to the confirm step so the surface stays
      // dismissable and the user can retry.
      setPhase('confirm')
      return
    }

    // Staged successfully — switch into the forced restarting state and let the
    // daemon + GUI restart so the migration lands on boot. From here we never
    // drop back to `confirm`: the bundle is already staged, so a restart hiccup
    // must not be confused with a staging failure. restartApp() exits this
    // process, so code after it is unreachable on the happy path.
    setStagedResult(result)
    setPhase('restarting')
    setPassword('')
    onStaged?.(result)
    try {
      await commands.restartDaemon()
      await commands.restartApp()
    } catch (error) {
      // Staging already succeeded; the migration applies on the next boot
      // regardless. Surface the restart failure but stay in `restarting` so the
      // user is told to relaunch manually rather than being sent back to retry
      // an import that is already committed.
      log.error({ err: error }, 'Failed to restart after staging config import')
      onError?.('generic', error)
    } finally {
      setBusy(false)
    }
  }

  const back = () => {
    if (isRestarting) return
    setPreview(null)
    setPhase('password')
  }

  const reset = () => {
    if (isRestarting) return
    setPhase('idle')
    setSourcePath(null)
    setPassword('')
    setPreview(null)
  }

  return {
    phase,
    sourcePath,
    password,
    setPassword,
    preview,
    stagedResult,
    busy,
    isRestarting,
    pickFile,
    submitPassword,
    confirmImport,
    back,
    reset,
  }
}
