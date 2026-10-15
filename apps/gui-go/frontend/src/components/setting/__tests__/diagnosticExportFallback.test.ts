import { describe, expect, it } from 'vitest'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import {
  normalizeOfflineExportResult,
  parseExportFailureCause,
} from '../general/diagnosticExportFallback'

function exportFailure(body: unknown): DaemonApiError {
  return new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '500 on /x', body, 500)
}

describe('parseExportFailureCause', () => {
  it.each([
    ['source_read_failed', 'sourceReadFailed'],
    ['archive_write_failed', 'archiveWriteFailed'],
    ['destination_unavailable', 'destinationUnavailable'],
    ['unknown', 'unknown'],
  ])('maps daemon cause %s', (wire, expected) => {
    const err = exportFailure({
      code: 'diagnostic_export_failed',
      message: 'm',
      details: { cause: wire },
    })
    expect(parseExportFailureCause(err)).toBe(expected)
  })

  it('maps an unrecognized cause under the export error code to unknown', () => {
    const err = exportFailure({ code: 'diagnostic_export_failed', details: { cause: 'weird' } })
    expect(parseExportFailureCause(err)).toBe('unknown')
  })

  it('returns undefined for other errors', () => {
    expect(parseExportFailureCause(new Error('network'))).toBeUndefined()
    expect(parseExportFailureCause(exportFailure({ code: 'other' }))).toBeUndefined()
  })
})

describe('normalizeOfflineExportResult', () => {
  it('maps a cancelled export to no path and no gaps', () => {
    expect(normalizeOfflineExportResult(null)).toEqual({ path: null, missingSources: [] })
  })

  it('carries the path and missing sources of a saved export', () => {
    expect(
      normalizeOfflineExportResult({
        path: '/a.zip',
        missingSources: [{ source: 'engine', reason: 'missing' }],
      })
    ).toEqual({ path: '/a.zip', missingSources: [{ source: 'engine', reason: 'missing' }] })
  })

  it('treats a null missingSources list as none', () => {
    expect(normalizeOfflineExportResult({ path: '/a.zip', missingSources: null })).toEqual({
      path: '/a.zip',
      missingSources: [],
    })
  })
})
