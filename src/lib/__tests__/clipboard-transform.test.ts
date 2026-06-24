import { describe, expect, it } from 'vitest'
import type { ClipboardEntryDto } from '@/api/daemon/clipboard'
import { projectClipboardEntry } from '../clipboard-transform'
import { getItemPreview } from '../clipboard-utils'

function makeDto(overrides: Partial<ClipboardEntryDto> = {}): ClipboardEntryDto {
  return {
    id: 'entry-1',
    preview: 'hello',
    hasDetail: false,
    sizeBytes: 5,
    capturedAt: 1,
    contentType: 'text/plain',
    thumbnailUrl: null,
    isEncrypted: false,
    isFavorited: false,
    updatedAt: 2,
    activeTime: 3,
    fileTransferStatus: null,
    fileTransferReason: null,
    linkUrls: null,
    linkDomains: null,
    fileSizes: null,
    ...overrides,
  }
}

describe('projectClipboardEntry', () => {
  it('projects text DTOs with raw timestamps and favorite flag', () => {
    const entry = projectClipboardEntry(makeDto({ isFavorited: true }))

    expect(entry).toEqual({
      id: 'entry-1',
      type: 'text',
      content: { display_text: 'hello', has_detail: false, size: 5 },
      createdAt: 1,
      updatedAt: 2,
      activeTime: 3,
      isFavorited: true,
      isUnavailable: false,
    })
  })

  it('preserves image dimensions from daemon DTOs', () => {
    const entry = projectClipboardEntry(
      makeDto({
        preview: 'Image (123 bytes)',
        sizeBytes: 123,
        contentType: 'image/png',
        imageWidth: 1920,
        imageHeight: 1080,
      })
    )

    expect(entry.type).toBe('image')
    expect(entry.content).toEqual({
      thumbnail: null,
      size: 123,
      width: 1920,
      height: 1080,
    })
    expect(getItemPreview(entry)).toBe('Image | 1920×1080 | 123 B')
  })

  it('parses file URI lists, keeping per-file missing flags', () => {
    const entry = projectClipboardEntry(
      makeDto({
        preview: 'file:///tmp/report.pdf\nuniclip-missing:///lost.bin?size=42',
        contentType: 'text/uri-list',
        fileSizes: [100, 42],
      })
    )

    expect(entry.type).toBe('file')
    expect(entry.content).toEqual({
      file_names: ['report.pdf', 'lost.bin'],
      file_sizes: [100, 42],
      file_missing: [false, true],
      file_paths: ['/tmp/report.pdf', null],
    })
  })

  it('projects link DTOs, deriving domains when the daemon omits them', () => {
    const entry = projectClipboardEntry(
      makeDto({
        preview: 'https://example.com/a',
        linkUrls: ['https://example.com/a'],
        linkDomains: null,
      })
    )

    expect(entry.type).toBe('link')
    expect(entry.content).toEqual({
      urls: ['https://example.com/a'],
      domains: ['example.com'],
    })
  })

  it('maps payloadState=Lost to isUnavailable so the row can grey out', () => {
    const entry = projectClipboardEntry(
      makeDto({
        preview: 'file:///tmp/gone.png',
        contentType: 'text/uri-list',
        payloadState: 'Lost',
      })
    )

    expect(entry.isUnavailable).toBe(true)
  })

  it('defaults isUnavailable to false when daemon omits payloadState', () => {
    expect(projectClipboardEntry(makeDto()).isUnavailable).toBe(false)
  })
})
