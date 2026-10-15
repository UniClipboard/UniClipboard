import { DaemonApiError } from '@/api/daemon/errors'

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

/**
 * Accepts the legacy bare path (`string | null`) as well as a structured result
 * carrying `missingSources`. Malformed entries are dropped.
 */
export function normalizeOfflineExportResult(raw: unknown): OfflineExportResult {
  if (typeof raw === 'string') return { path: raw, missingSources: [] }
  if (typeof raw !== 'object' || raw === null) return { path: null, missingSources: [] }
  const { path, missingSources } = raw as { path?: unknown; missingSources?: unknown }
  return {
    path: typeof path === 'string' ? path : null,
    missingSources: Array.isArray(missingSources)
      ? missingSources.flatMap(item => {
          const { source, reason } = (item ?? {}) as { source?: unknown; reason?: unknown }
          return typeof source === 'string' && typeof reason === 'string'
            ? [{ source, reason }]
            : []
        })
      : [],
  }
}
