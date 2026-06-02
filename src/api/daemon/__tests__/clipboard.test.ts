/**
 * Unit tests for the daemon clipboard API module.
 *
 * These tests verify type correctness, HTTP method contracts, and basic API function signatures.
 * Integration tests against a running daemon would require mocking the HTTP layer.
 */

import { describe, expect, it, vi } from 'vitest'
import type { ClipboardEntryDto, ClipboardEntriesResponse, ClipboardStats } from '../clipboard'

// ADR-008 P7: clipboard wrappers route through the generated SDK + `callSdk`.
// - `../client` exposes a `callSdk` mock that replicates the happy path:
//   invoke the SDK thunk and unwrap its outer `{ data }` (= ApiEnvelope).
// - The generated SDK fns are mocked so each test controls the returned
//   `{ data: <envelope> }` and can assert path/query/body/throwOnError.
vi.mock('../client', () => ({
  daemonClient: {
    request: vi.fn(),
    callSdk: vi.fn((call: () => Promise<{ data: unknown }>) => call().then(r => r.data)),
    initialized: true,
  },
}))

vi.mock('../../generated/sdk.gen', () => ({
  listClipboardEntries: vi.fn(),
  getClipboardEntry: vi.fn(),
  deleteClipboardEntry: vi.fn(),
  restoreClipboardEntry: vi.fn(),
  toggleClipboardEntryFavorite: vi.fn(),
  clearClipboardHistory: vi.fn(),
  getClipboardStats: vi.fn(),
  getClipboardEntryResource: vi.fn(),
}))

// ── Type tests ───────────────────────────────────────────────────

describe('ClipboardEntryDto type', () => {
  it('accepts a valid entry projection', () => {
    const entry: ClipboardEntryDto = {
      id: 'entry-123',
      preview: 'Hello, World!',
      hasDetail: true,
      sizeBytes: 13,
      capturedAt: 1710000000000,
      contentType: 'text/plain',
      thumbnailUrl: null,
      isEncrypted: false,
      isFavorited: false,
      updatedAt: 1710000000000,
      activeTime: 1710000000000,
      fileTransferStatus: null,
      fileTransferReason: null,
      linkUrls: null,
      linkDomains: null,
      fileSizes: null,
    }

    expect(entry.id).toBe('entry-123')
    expect(entry.preview).toBe('Hello, World!')
    expect(entry.sizeBytes).toBe(13)
  })

  it('accepts entry with link data', () => {
    const entry: ClipboardEntryDto = {
      id: 'entry-link-1',
      preview: 'https://example.com',
      hasDetail: true,
      sizeBytes: 19,
      capturedAt: 1710000000000,
      contentType: 'text/uri-list',
      thumbnailUrl: null,
      isEncrypted: false,
      isFavorited: true,
      updatedAt: 1710000000000,
      activeTime: 1710000000000,
      fileTransferStatus: null,
      fileTransferReason: null,
      linkUrls: ['https://example.com/path'],
      linkDomains: ['example.com'],
      fileSizes: null,
    }

    expect(entry.linkUrls).toHaveLength(1)
    expect(entry.linkDomains).toEqual(['example.com'])
  })

  it('accepts entry with file transfer status', () => {
    const entry: ClipboardEntryDto = {
      id: 'entry-file-1',
      preview: 'file:///path/to/document.pdf',
      hasDetail: true,
      sizeBytes: 102400,
      capturedAt: 1710000000000,
      contentType: 'text/uri-list',
      thumbnailUrl: null,
      isEncrypted: false,
      isFavorited: false,
      updatedAt: 1710000000000,
      activeTime: 1710000000000,
      fileTransferStatus: 'completed',
      fileTransferReason: null,
      linkUrls: null,
      linkDomains: null,
      fileSizes: [102400],
    }

    expect(entry.fileTransferStatus).toBe('completed')
    expect(entry.fileSizes).toEqual([102400])
  })
})

describe('ClipboardEntriesResponse type', () => {
  it('accepts ready status with entries', () => {
    const response: ClipboardEntriesResponse = {
      status: 'ready',
      entries: [
        {
          id: 'entry-1',
          preview: 'Test content',
          hasDetail: true,
          sizeBytes: 12,
          capturedAt: 1710000000000,
          contentType: 'text/plain',
          thumbnailUrl: null,
          isEncrypted: false,
          isFavorited: false,
          updatedAt: 1710000000000,
          activeTime: 1710000000000,
          fileTransferStatus: null,
          fileTransferReason: null,
          linkUrls: null,
          linkDomains: null,
          fileSizes: null,
        },
      ],
    }

    expect(response.status).toBe('ready')
    expect(response.entries).toHaveLength(1)
  })

  it('accepts not_ready status', () => {
    const response: ClipboardEntriesResponse = {
      status: 'not_ready',
    }

    expect(response.status).toBe('not_ready')
    expect(response.entries).toBeUndefined()
  })
})

describe('ClipboardStats type', () => {
  it('accepts valid stats', () => {
    const stats: ClipboardStats = {
      totalItems: 42,
      totalSize: 1024000,
    }

    expect(stats.totalItems).toBe(42)
    expect(stats.totalSize).toBe(1024000)
  })

  it('accepts zero stats', () => {
    const stats: ClipboardStats = {
      totalItems: 0,
      totalSize: 0,
    }

    expect(stats.totalItems).toBe(0)
    expect(stats.totalSize).toBe(0)
  })
})

// ── API function contract tests ──────────────────────────────────

describe('toggleFavorite HTTP contract', () => {
  it('calls toggleClipboardEntryFavorite with the entry id path param', async () => {
    const { toggleClipboardEntryFavorite } = await import('../../generated/sdk.gen')
    const { toggleFavorite } = await import('../clipboard')

    vi.mocked(toggleClipboardEntryFavorite).mockResolvedValue({
      data: { data: { success: true }, ts: 0 },
    } as never)

    await toggleFavorite('entry-123', true)

    expect(toggleClipboardEntryFavorite).toHaveBeenCalledWith(
      expect.objectContaining({ path: { id: 'entry-123' }, throwOnError: true })
    )
  })

  it('sends isFavorited in request body', async () => {
    const { toggleClipboardEntryFavorite } = await import('../../generated/sdk.gen')
    const { toggleFavorite } = await import('../clipboard')

    vi.mocked(toggleClipboardEntryFavorite).mockResolvedValue({
      data: { data: { success: true }, ts: 0 },
    } as never)

    await toggleFavorite('entry-123', true)

    expect(toggleClipboardEntryFavorite).toHaveBeenCalledWith(
      expect.objectContaining({ body: { isFavorited: true } })
    )
  })
})

describe('clearClipboardHistory HTTP contract', () => {
  it('calls clearClipboardHistory SDK fn with throwOnError', async () => {
    const { clearClipboardHistory: clearClipboardHistorySdk } =
      await import('../../generated/sdk.gen')
    const { clearClipboardHistory } = await import('../clipboard')

    vi.mocked(clearClipboardHistorySdk).mockResolvedValue({
      data: { data: { deletedCount: 5, failedEntries: [] }, ts: 0 },
    } as never)

    const result = await clearClipboardHistory()

    expect(clearClipboardHistorySdk).toHaveBeenCalledWith(
      expect.objectContaining({ throwOnError: true })
    )
    expect(result.deletedCount).toBe(5)
    expect(result.failedEntries).toEqual([])
  })

  it('returns result with failed entries when some deletions fail', async () => {
    const { clearClipboardHistory: clearClipboardHistorySdk } =
      await import('../../generated/sdk.gen')
    const { clearClipboardHistory } = await import('../clipboard')

    vi.mocked(clearClipboardHistorySdk).mockResolvedValue({
      data: {
        data: {
          deletedCount: 3,
          failedEntries: [
            ['entry-4', 'database error'],
            ['entry-5', 'not found'],
          ],
        },
        ts: 0,
      },
    } as never)

    const result = await clearClipboardHistory()

    expect(result.deletedCount).toBe(3)
    expect(result.failedEntries).toHaveLength(2)
  })
})

describe('getEntryDetail HTTP contract', () => {
  it('calls getClipboardEntry SDK fn with the entry id path param', async () => {
    const { getClipboardEntry } = await import('../../generated/sdk.gen')
    const { getEntryDetail } = await import('../clipboard')

    vi.mocked(getClipboardEntry).mockResolvedValue({
      data: {
        data: {
          id: 'entry-123',
          content: 'Hello, World!',
          sizeBytes: 13,
          createdAtMs: 1710000000000,
          activeTimeMs: 1710000000000,
          mimeType: 'text/plain',
        },
        ts: 0,
      },
    } as never)

    const result = await getEntryDetail('entry-123')

    expect(getClipboardEntry).toHaveBeenCalledWith(
      expect.objectContaining({ path: { id: 'entry-123' }, throwOnError: true })
    )
    expect(result?.content).toBe('Hello, World!')
  })

  it('returns null on not-found error', async () => {
    const { getClipboardEntry } = await import('../../generated/sdk.gen')
    const { getEntryDetail } = await import('../clipboard')

    const notFoundError = new Error('not found')
    ;(notFoundError as unknown as { code: string }).code = 'NOT_FOUND'
    vi.mocked(getClipboardEntry).mockRejectedValue(notFoundError)

    const result = await getEntryDetail('nonexistent-id')

    expect(result).toBeNull()
  })

  it('re-throws non-not-found errors', async () => {
    const { getClipboardEntry } = await import('../../generated/sdk.gen')
    const { getEntryDetail } = await import('../clipboard')

    const serverError = new Error('server error')
    vi.mocked(getClipboardEntry).mockRejectedValue(serverError)

    await expect(getEntryDetail('entry-123')).rejects.toThrow('server error')
  })
})

describe('getClipboardEntryResource HTTP contract', () => {
  it('calls getClipboardEntryResource SDK fn with the entry id path param', async () => {
    const { getClipboardEntryResource: getClipboardEntryResourceSdk } =
      await import('../../generated/sdk.gen')
    const { getClipboardEntryResource } = await import('../clipboard')

    vi.mocked(getClipboardEntryResourceSdk).mockResolvedValue({
      data: {
        data: {
          blobId: 'blob-abc',
          mimeType: 'text/plain',
          sizeBytes: 1024,
          url: null,
          inlineData: 'Hello World',
        },
        ts: 0,
      },
    } as never)

    const result = await getClipboardEntryResource('entry-123')

    expect(getClipboardEntryResourceSdk).toHaveBeenCalledWith(
      expect.objectContaining({ path: { id: 'entry-123' }, throwOnError: true })
    )
    expect(result?.blobId).toBe('blob-abc')
  })

  it('returns null on not-found', async () => {
    const { getClipboardEntryResource: getClipboardEntryResourceSdk } =
      await import('../../generated/sdk.gen')
    const { getClipboardEntryResource } = await import('../clipboard')

    const notFoundError = new Error('not found')
    ;(notFoundError as unknown as { code: string }).code = 'NOT_FOUND'
    vi.mocked(getClipboardEntryResourceSdk).mockRejectedValue(notFoundError)

    const result = await getClipboardEntryResource('nonexistent-id')

    expect(result).toBeNull()
  })
})

describe('deleteClipboardEntry HTTP contract', () => {
  it('calls deleteClipboardEntry SDK fn with the entry id path param', async () => {
    const { deleteClipboardEntry: deleteClipboardEntrySdk } =
      await import('../../generated/sdk.gen')
    const { deleteClipboardEntry } = await import('../clipboard')

    // 204 endpoint — the SDK resolves to `{ data: undefined }`; the wrapper
    // ignores the payload entirely.
    vi.mocked(deleteClipboardEntrySdk).mockResolvedValue({ data: undefined } as never)

    await deleteClipboardEntry('entry-123')

    expect(deleteClipboardEntrySdk).toHaveBeenCalledWith(
      expect.objectContaining({ path: { id: 'entry-123' }, throwOnError: true })
    )
  })
})
