import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ClipboardEntry, ClipboardEntryContent } from '@/lib/clipboard-entry'
import {
  fileUriToLocalPath,
  firstRevealableFilePath,
  formatRelativeTime,
  getItemPreview,
} from '../clipboard-utils'

function createEntry(
  type: ClipboardEntry['type'],
  content: ClipboardEntryContent | null
): ClipboardEntry {
  return {
    id: 'item-1',
    type,
    content,
    createdAt: 0,
    updatedAt: 0,
    activeTime: 0,
    isFavorited: false,
    isUnavailable: false,
  }
}

describe('clipboard-utils', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-03-16T00:00:00Z'))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('returns preview text for each supported entry type', () => {
    expect(
      getItemPreview(createEntry('text', { display_text: 'hello', has_detail: true, size: 5 }))
    ).toBe('hello')
    expect(
      getItemPreview(createEntry('image', { thumbnail: null, size: 1, width: 1, height: 1 }))
    ).toBe('Image | 1×1 | 1 B')
    expect(
      getItemPreview(createEntry('image', { thumbnail: null, size: 0, width: 0, height: 0 }))
    ).toBe('Image')
    expect(
      getItemPreview(createEntry('link', { urls: ['https://a.test'], domains: ['a.test'] }))
    ).toBe('https://a.test')
    expect(getItemPreview(createEntry('file', { file_names: ['a.txt'], file_sizes: [1] }))).toBe(
      'a.txt'
    )
    expect(getItemPreview(createEntry('code', { code: 'const x = 1' }))).toBe('const x = 1')
    expect(getItemPreview(createEntry('unknown', null))).toBe('')
  })

  it('formats relative time using quick-panel rules', () => {
    const now = Date.now()
    expect(formatRelativeTime(now)).toBe('just now')
    expect(formatRelativeTime(now - 5 * 60000)).toBe('5m')
    expect(formatRelativeTime(now - 2 * 3600000)).toBe('2h')
    expect(formatRelativeTime(now - 3 * 86400000)).toBe('3d')
  })
})

describe('fileUriToLocalPath', () => {
  it('decodes a POSIX file:// URI, including percent-encoded segments', () => {
    expect(fileUriToLocalPath('file:///tmp/report.pdf')).toBe('/tmp/report.pdf')
    expect(fileUriToLocalPath('file:///tmp/my%20file.txt')).toBe('/tmp/my file.txt')
    expect(fileUriToLocalPath('  file:///tmp/spaced.bin  ')).toBe('/tmp/spaced.bin')
  })

  it('strips the leading slash before a Windows drive letter', () => {
    expect(fileUriToLocalPath('file:///C:/dir/f.txt')).toBe('C:/dir/f.txt')
  })

  it('returns null for non-file URIs and unparseable input', () => {
    expect(fileUriToLocalPath('uniclip-missing:///lost.bin?size=42')).toBeNull()
    expect(fileUriToLocalPath('https://example.com/a')).toBeNull()
    expect(fileUriToLocalPath('')).toBeNull()
  })
})

describe('firstRevealableFilePath', () => {
  it('returns the first non-missing decoded path', () => {
    expect(
      firstRevealableFilePath({
        file_names: ['a', 'b'],
        file_sizes: [1, 2],
        file_paths: ['/tmp/a', '/tmp/b'],
        file_missing: [false, false],
      })
    ).toBe('/tmp/a')
  })

  it('skips missing files and null paths', () => {
    expect(
      firstRevealableFilePath({
        file_names: ['gone', 'ok'],
        file_sizes: [1, 2],
        file_paths: [null, '/tmp/ok'],
        file_missing: [true, false],
      })
    ).toBe('/tmp/ok')
  })

  it('returns null when no file has a usable path', () => {
    expect(
      firstRevealableFilePath({
        file_names: ['gone'],
        file_sizes: [1],
        file_paths: [null],
        file_missing: [true],
      })
    ).toBeNull()
    // Historical entry without the file_paths field.
    expect(firstRevealableFilePath({ file_names: ['legacy'], file_sizes: [1] })).toBeNull()
    // Non-file content.
    expect(firstRevealableFilePath({ display_text: 'hi', has_detail: false, size: 2 })).toBeNull()
    expect(firstRevealableFilePath(null)).toBeNull()
  })
})
