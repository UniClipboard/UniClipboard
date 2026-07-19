import { describe, expect, it } from 'vitest'
import type { ClipboardEntryDto } from '@/api/daemon/clipboard'
import type { SearchResultDto } from '@/api/daemon/search'
import {
  clipboardEntryToDisplayItem,
  projectClipboardEntry,
  searchResultToDisplayItem,
} from '../clipboard-transform'
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
    contentTags: null,
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
      isDirectory: false,
    })
  })

  it('projects the directory flag from a file DTO', () => {
    const dir = projectClipboardEntry(
      makeDto({ contentType: 'file', preview: 'file:///root/child', isDirectory: true })
    )
    expect(dir.isDirectory).toBe(true)

    // Absent flag (older daemon) and non-directory files fall back to false.
    expect(projectClipboardEntry(makeDto({ contentType: 'file' })).isDirectory).toBe(false)
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
    // The daemon list projection carries the persisted content category
    // (`file`), not a representation MIME, so the file branch must key off
    // that label rather than the legacy `text/uri-list` string.
    const entry = projectClipboardEntry(
      makeDto({
        preview: 'file:///tmp/report.pdf\nuniclip-missing:///lost.bin?size=42',
        contentType: 'file',
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
        preview: 'https://example.com/a?truncated',
        linkUrls: ['https://example.com/a'],
        linkDomains: null,
      })
    )

    expect(entry.type).toBe('text')
    expect(entry.contentTags).toEqual(['link'])
    expect(entry.content).toEqual({
      display_text: 'https://example.com/a?truncated',
      has_detail: false,
      size: 5,
      link_urls: ['https://example.com/a'],
    })
  })

  it('honors browse contentTags from the daemon', () => {
    const entry = projectClipboardEntry(
      makeDto({
        preview: 'function greet(name) {\n  return name\n}',
        contentTags: ['code'],
      })
    )

    expect(entry.type).toBe('text')
    expect(entry.contentTags).toEqual(['code'])
  })

  it('surfaces the other physical category as unknown, not text', () => {
    const entry = projectClipboardEntry(
      makeDto({
        preview: 'opaque payload',
        contentType: 'other',
      })
    )

    expect(entry.type).toBe('unknown')
    expect(entry.contentTags).toBeUndefined()
  })

  it('projects rich text entries without the code tag', () => {
    const entry = projectClipboardEntry(
      makeDto({
        preview: '<pre>let x = 1</pre>',
        contentType: 'rich_text',
      })
    )

    expect(entry.type).toBe('richtext')
    expect(entry.contentTags).toBeUndefined()
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

function makeSearchResult(overrides: Partial<SearchResultDto> = {}): SearchResultDto {
  return {
    entryId: 'result-1',
    contentType: 'text',
    activeTimeMs: 1,
    tags: [],
    textPreview: 'hello',
    charCount: null,
    mimeType: 'text/plain',
    fileExtensions: [],
    fileNames: [],
    filePaths: [],
    linkUrls: [],
    sourceDevice: null,
    payloadState: null,
    ...overrides,
  }
}

describe('searchResultToDisplayItem', () => {
  it('keeps link search results tagged as links', () => {
    const item = searchResultToDisplayItem(
      makeSearchResult({
        tags: ['link'],
        textPreview: 'https://example.com/a?preview',
        linkUrls: ['https://example.com/a'],
      })
    )

    expect(item.type).toBe('text')
    expect(item.contentTags).toEqual(['link'])
    expect(item.content).toMatchObject({
      display_text: 'https://example.com/a?preview',
      link_urls: ['https://example.com/a'],
    })
  })

  it('maps HTML search results to rich text without the code tag', () => {
    const item = searchResultToDisplayItem(
      makeSearchResult({
        contentType: 'html',
        textPreview: '<pre>let x = 1</pre>',
        mimeType: 'text/html',
      })
    )

    expect(item.type).toBe('richtext')
    expect(item.contentTags).toBeUndefined()
  })

  it('maps file search results to file content carrying names and local paths', () => {
    const item = searchResultToDisplayItem(
      makeSearchResult({
        contentType: 'file',
        mimeType: 'text/uri-list',
        fileNames: ['report.pdf'],
        filePaths: ['/tmp/report.pdf'],
      })
    )

    expect(item.type).toBe('file')
    expect(item.content).toMatchObject({
      file_names: ['report.pdf'],
      file_paths: ['/tmp/report.pdf'],
    })
  })

  it('projects the directory flag from the builtin directory tag', () => {
    // The search projection derives the `directory` builtin tag; the display
    // item must key its status-only directory-send row off it. Regression for
    // the flag being dropped between SearchResultDto and the rendered item.
    const dir = searchResultToDisplayItem(
      makeSearchResult({ contentType: 'file', tags: ['directory'], fileNames: ['photos'] })
    )
    expect(dir.isDirectory).toBe(true)

    // A flat file result (no directory tag) stays non-directory.
    const flat = searchResultToDisplayItem(makeSearchResult({ contentType: 'file' }))
    expect(flat.isDirectory).toBe(false)
  })
})

describe('clipboardEntryToDisplayItem', () => {
  it('carries the directory flag from the browse entry into the display item', () => {
    // The realtime browse path (useLiveSearch.onLocalItem) runs this over a
    // freshly-captured entry. Regression for issue #1330: the flag was dropped
    // by an inline field copy, so directory sends still showed a byte %.
    const entry = projectClipboardEntry(
      makeDto({ contentType: 'file', preview: 'file:///root/child', isDirectory: true })
    )
    expect(clipboardEntryToDisplayItem(entry).isDirectory).toBe(true)

    const flat = projectClipboardEntry(makeDto({ contentType: 'file' }))
    expect(clipboardEntryToDisplayItem(flat).isDirectory).toBe(false)
  })

  it('preserves the render-relevant fields', () => {
    const entry = projectClipboardEntry(
      makeDto({ id: 'e9', preview: 'hi', isFavorited: true, payloadState: 'Lost' })
    )
    const item = clipboardEntryToDisplayItem(entry)
    expect(item).toMatchObject({
      id: 'e9',
      type: entry.type,
      content: entry.content,
      activeTime: entry.activeTime,
      isFavorited: true,
      isUnavailable: true,
    })
  })
})
