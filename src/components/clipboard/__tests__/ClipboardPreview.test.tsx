import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import ClipboardPreview from '@/components/clipboard/ClipboardPreview'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: vi.fn((path: string) => `asset://localhost/${encodeURIComponent(path)}`),
}))

// Resolve daemon blob paths to a deterministic object URL so `<img src>` is
// assertable without standing up the real fetch + URL.createObjectURL path.
vi.mock('@/api/daemon/blob-image-cache', () => ({
  getBlobImageObjectUrl: vi.fn(async (path: string) => `blob:mock-${path}`),
}))

const previewMock = vi.hoisted((): { value: ClipboardPreviewData | null } => ({
  value: {
    entryId: 'image-file-entry',
    contentType: 'image',
    sizeBytes: 2048,
    imageBlobPath: '/clipboard/blobs/blob-image',
  },
}))

vi.mock('@/hooks/useClipboardPreviewState', () => ({
  useClipboardPreviewState: () => ({
    effectiveStatus: 'completed',
    entryStatus: undefined,
    imageDimensions: null,
    loading: false,
    preview: previewMock.value,
    setImageDimensions: vi.fn(),
    transfer: undefined,
  }),
}))

vi.mock('@/hooks/useEntryDelivery', () => ({
  useEntryDelivery: () => ({ delivery: null, loading: false, error: null }),
}))

vi.mock('@/api/file_transfer', () => ({
  cancelFileTransfer: vi.fn(),
}))

function createImageFileItem(
  fileName = 'screenshot.png',
  overrides: Partial<DisplayClipboardItem> = {}
): DisplayClipboardItem {
  return {
    id: 'image-file-entry',
    type: 'file',
    activeTime: 1710000000000,
    content: {
      file_names: [fileName],
      file_sizes: [2048],
    },
    ...overrides,
  }
}

function createPdfFileItem(): DisplayClipboardItem {
  return {
    id: 'pdf-file-entry',
    type: 'file',
    activeTime: 1710000000000,
    content: {
      file_names: ['report.pdf'],
      file_sizes: [2048],
    },
  }
}

describe('ClipboardPreview', () => {
  beforeEach(() => {
    previewMock.value = {
      entryId: 'image-file-entry',
      contentType: 'image',
      sizeBytes: 2048,
      imageBlobPath: '/clipboard/blobs/blob-image',
    }
  })

  it('renders a direct image preview with filename for image files', async () => {
    render(<ClipboardPreview item={createImageFileItem()} />)

    // The `<img>` resolves its object URL asynchronously via the cache hook.
    const image = await screen.findByRole('img', { name: 'screenshot.png' })
    await waitFor(() =>
      expect(image).toHaveAttribute('src', 'blob:mock-/clipboard/blobs/blob-image')
    )
    expect(image.closest('.max-w-sm')).not.toBeInTheDocument()
    expect(screen.getByText('screenshot.png')).toBeInTheDocument()
  })

  it('wraps long image file names instead of truncating them', () => {
    const longName =
      'very-long-screenshot-name-with-window-title-and-device-label-that-should-wrap.png'

    render(<ClipboardPreview item={createImageFileItem(longName)} />)

    const fileName = screen.getByText(longName)
    expect(fileName).not.toHaveClass('truncate')
    expect(fileName).toHaveClass('break-all')
  })

  it('renders multiple image files as an image grid with complete filenames', () => {
    const longName =
      'very-long-screenshot-name-with-window-title-and-device-label-that-should-wrap.png'
    const item = createImageFileItem('unused.png', {
      content: {
        file_names: ['first.png', longName, 'third.webp'],
        file_sizes: [2048, 4096, 8192],
        file_paths: ['/tmp/first.png', '/tmp/second.png', '/tmp/third.webp'],
      },
    })

    render(<ClipboardPreview item={item} />)

    const grid = screen.getByRole('list', { name: 'Image files' })
    expect(grid).toHaveClass('grid')
    expect(screen.getByRole('img', { name: 'first.png' })).toHaveAttribute(
      'src',
      expect.any(String)
    )
    expect(screen.getByRole('img', { name: longName })).toHaveAttribute('src', expect.any(String))
    expect(screen.getByRole('img', { name: 'third.webp' })).toHaveAttribute(
      'src',
      expect.any(String)
    )
    expect(screen.getByText('3 images')).toBeInTheDocument()
    expect(screen.getAllByText('14.00 KB').length).toBeGreaterThan(0)

    const fileName = screen.getByText(longName)
    expect(fileName).not.toHaveClass('truncate')
    expect(fileName).toHaveClass('break-all')
  })

  it('renders every image in a local image group through Tauri asset URLs', () => {
    const firstPath = '/Users/mark/Downloads/ChatGPT Image 2026年5月3日 10_45_33.png'
    const secondPath = '/Users/mark/Downloads/ChatGPT Image 2026年5月3日 10_45_39.png'
    const item = createImageFileItem('unused.png', {
      content: {
        file_names: [
          'ChatGPT Image 2026年5月3日 10_45_33.png',
          'ChatGPT Image 2026年5月3日 10_45_39.png',
        ],
        file_sizes: [2048, 4096],
        file_paths: [firstPath, secondPath],
      },
    })

    render(<ClipboardPreview item={item} />)

    const images = screen.getAllByRole('img')
    expect(images).toHaveLength(2)
    for (const image of images) {
      const src = image.getAttribute('src') ?? ''
      expect(src).toMatch(/^asset:\/\/localhost\//)
      expect(src).not.toMatch(/^\/Users\//)
    }
  })

  it('keeps mixed multi-file selections on the file list preview', () => {
    const item = createImageFileItem('unused.png', {
      content: {
        file_names: ['first.png', 'report.pdf'],
        file_sizes: [2048, 4096],
      },
    })

    render(<ClipboardPreview item={item} />)

    expect(screen.queryByRole('list', { name: 'Image files' })).not.toBeInTheDocument()
    expect(screen.queryByRole('img', { name: 'first.png' })).not.toBeInTheDocument()
    expect(screen.getByText('first.png')).toBeInTheDocument()
    expect(screen.getByText('report.pdf')).toBeInTheDocument()
  })

  it('keeps non-image files on the file card preview', () => {
    render(<ClipboardPreview item={createPdfFileItem()} />)

    expect(screen.queryByRole('img', { name: 'report.pdf' })).not.toBeInTheDocument()
    expect(screen.getByText('report.pdf')).toBeInTheDocument()
  })
})
