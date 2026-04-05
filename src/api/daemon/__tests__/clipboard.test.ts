/**
 * Unit tests for the daemon clipboard API module.
 *
 * These tests verify type correctness, HTTP method contracts, and basic API function signatures.
 * Integration tests against a running daemon would require mocking the HTTP layer.
 */

import { describe, expect, it, vi } from 'vitest'
import type { ClipboardEntryDto, ClipboardEntriesResponse, ClipboardStats } from '../clipboard'

// Mock the daemonClient
vi.mock('../client', () => ({
  daemonClient: {
    request: vi.fn(),
    initialized: true,
  },
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
  it('uses POST method as defined by daemon route', async () => {
    const { daemonClient } = await import('../client')
    const { toggleFavorite } = await import('../clipboard')

    // Mock successful response
    vi.mocked(daemonClient.request).mockResolvedValue(undefined)

    await toggleFavorite('entry-123', true)

    expect(daemonClient.request).toHaveBeenCalledWith(
      '/clipboard/entries/entry-123/favorite',
      expect.objectContaining({ method: 'POST' })
    )
  })

  it('sends isFavorited in request body', async () => {
    const { daemonClient } = await import('../client')
    const { toggleFavorite } = await import('../clipboard')

    vi.mocked(daemonClient.request).mockResolvedValue(undefined)

    await toggleFavorite('entry-123', true)

    expect(daemonClient.request).toHaveBeenCalledWith(
      '/clipboard/entries/entry-123/favorite',
      expect.objectContaining({
        body: { isFavorited: true },
      })
    )
  })
})

describe('clearClipboardHistory HTTP contract', () => {
  it('uses POST method to /clipboard/entries/clear', async () => {
    const { daemonClient } = await import('../client')
    const { clearClipboardHistory } = await import('../clipboard')

    vi.mocked(daemonClient.request).mockResolvedValue({
      data: { deletedCount: 5, failedEntries: [] },
      ts: Date.now(),
    })

    const result = await clearClipboardHistory()

    expect(daemonClient.request).toHaveBeenCalledWith(
      '/clipboard/entries/clear',
      expect.objectContaining({ method: 'POST' })
    )
    expect(result.deletedCount).toBe(5)
    expect(result.failedEntries).toEqual([])
  })

  it('returns result with failed entries when some deletions fail', async () => {
    const { daemonClient } = await import('../client')
    const { clearClipboardHistory } = await import('../clipboard')

    vi.mocked(daemonClient.request).mockResolvedValue({
      data: {
        deletedCount: 3,
        failedEntries: [
          ['entry-4', 'database error'],
          ['entry-5', 'not found'],
        ],
      },
      ts: Date.now(),
    })

    const result = await clearClipboardHistory()

    expect(result.deletedCount).toBe(3)
    expect(result.failedEntries).toHaveLength(2)
  })
})

describe('getEntryDetail HTTP contract', () => {
  it('uses GET method to /clipboard/entries/:id', async () => {
    const { daemonClient } = await import('../client')
    const { getEntryDetail } = await import('../clipboard')

    vi.mocked(daemonClient.request).mockResolvedValue({
      data: {
        id: 'entry-123',
        content: 'Hello, World!',
        sizeBytes: 13,
        createdAtMs: 1710000000000,
        activeTimeMs: 1710000000000,
        mimeType: 'text/plain',
      },
      ts: Date.now(),
    })

    const result = await getEntryDetail('entry-123')

    expect(daemonClient.request).toHaveBeenCalledWith('/clipboard/entries/entry-123')
    expect(result?.content).toBe('Hello, World!')
  })

  it('returns null on not-found error', async () => {
    const { daemonClient } = await import('../client')
    const { getEntryDetail } = await import('../clipboard')

    const notFoundError = new Error('not found')
    ;(notFoundError as unknown as { code: string }).code = 'NOT_FOUND'
    vi.mocked(daemonClient.request).mockRejectedValue(notFoundError)

    const result = await getEntryDetail('nonexistent-id')

    expect(result).toBeNull()
  })

  it('re-throws non-not-found errors', async () => {
    const { daemonClient } = await import('../client')
    const { getEntryDetail } = await import('../clipboard')

    const serverError = new Error('server error')
    vi.mocked(daemonClient.request).mockRejectedValue(serverError)

    await expect(getEntryDetail('entry-123')).rejects.toThrow('server error')
  })
})

describe('getClipboardEntryResource HTTP contract', () => {
  it('uses GET to /clipboard/entries/:id/resource', async () => {
    const { daemonClient } = await import('../client')
    const { getClipboardEntryResource } = await import('../clipboard')

    vi.mocked(daemonClient.request).mockResolvedValue({
      data: {
        blobId: 'blob-abc',
        mimeType: 'text/plain',
        sizeBytes: 1024,
        url: null,
        inlineData: 'Hello World',
      },
      ts: Date.now(),
    })

    const result = await getClipboardEntryResource('entry-123')

    expect(daemonClient.request).toHaveBeenCalledWith('/clipboard/entries/entry-123/resource')
    expect(result?.blobId).toBe('blob-abc')
  })

  it('returns null on not-found', async () => {
    const { daemonClient } = await import('../client')
    const { getClipboardEntryResource } = await import('../clipboard')

    const notFoundError = new Error('not found')
    ;(notFoundError as unknown as { code: string }).code = 'NOT_FOUND'
    vi.mocked(daemonClient.request).mockRejectedValue(notFoundError)

    const result = await getClipboardEntryResource('nonexistent-id')

    expect(result).toBeNull()
  })
})

describe('deleteClipboardEntry HTTP contract', () => {
  it('uses DELETE method', async () => {
    const { daemonClient } = await import('../client')
    const { deleteClipboardEntry } = await import('../clipboard')

    vi.mocked(daemonClient.request).mockResolvedValue(undefined)

    await deleteClipboardEntry('entry-123')

    expect(daemonClient.request).toHaveBeenCalledWith(
      '/clipboard/entries/entry-123',
      expect.objectContaining({ method: 'DELETE' })
    )
  })
})
