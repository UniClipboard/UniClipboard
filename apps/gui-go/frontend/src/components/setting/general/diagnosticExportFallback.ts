import { DaemonApiError } from '@/api/daemon/errors'
import type { ExportStartupLogsResult } from '@/lib/ipc'

/**
 * Adapter for the offline (startup) log export contract. The Go host's
 * `exportStartupLogs` result and the daemon's `diagnostic_export_failed`
 * error details are normalized here so the UI depends on one shape.
 */

export type ExportFailureCause =
  | 'sourceReadFailed'
  | 'archiveWriteFailed'
  | 'destinationUnavailable'
  | 'unknown'

export interface MissingExportSource {
  /** `engine`, `daemon`, `gui`, `cli` or `quick-panel`; other values are shown verbatim. */
  source: string
  reason: string
}

export interface OfflineExportResult {
  path: string | null
  missingSources: MissingExportSource[]
}

const CAUSE_BY_WIRE: Record<string, ExportFailureCause> = {
  source_read_failed: 'sourceReadFailed',
  archive_write_failed: 'archiveWriteFailed',
  destination_unavailable: 'destinationUnavailable',
  unknown: 'unknown',
}

const SOURCE_LABEL_KEYS: Record<string, string> = {
  engine: 'engine',
  daemon: 'daemon',
  gui: 'gui',
  cli: 'cli',
  'quick-panel': 'quickPanel',
}

/** Locale key suffix (under `export.sources`) for a source, or `null` when unrecognized. */
export function sourceLabelKey(source: string): string | null {
  return SOURCE_LABEL_KEYS[source] ?? null
}

/** Read `details.cause` from a `diagnostic_export_failed` daemon error. */
export function parseExportFailureCause(error: unknown): ExportFailureCause | undefined {
  if (!(error instanceof DaemonApiError) || error.apiCode !== 'diagnostic_export_failed') {
    return undefined
  }
  const details = error.apiDetails
  const wire =
    typeof details === 'object' && details !== null
      ? (details as { cause?: unknown }).cause
      : undefined
  return (typeof wire === 'string' && CAUSE_BY_WIRE[wire]) || 'unknown'
}

/** Maps the host's `exportStartupLogs` result (`null` when the user cancelled) onto the UI shape. */
export function normalizeOfflineExportResult(
  result: ExportStartupLogsResult | null
): OfflineExportResult {
  return { path: result?.path ?? null, missingSources: result?.missingSources ?? [] }
}
