import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { daemonClient } from '@/api/daemon/client'
import ClipboardItem from '@/components/clipboard/ClipboardItem'

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    request: vi.fn(),
    blobUrl: vi.fn((path: string) => `http://127.0.0.1:12345${path}?auth=Session+test`),
  },
}))

const requestMock = vi.mocked(daemonClient.request)
const blobUrlMock = vi.mocked(daemonClient.blobUrl)

describe('ClipboardItem', () => {
  beforeEach(() => {
    requestMock.mockReset()
    blobUrlMock.mockImplementation(
      (path: string) => `http://127.0.0.1:12345${path}?auth=Session+test`
    )
    globalThis.fetch = vi.fn()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('expands by fetching resource bytes and decoding text', async () => {
    const preview = 'x'.repeat(260)
    const fullText = 'full content'
    const url = '/clipboard/blobs/blob-1'

    requestMock.mockResolvedValue({
      data: {
        blobId: 'blob-1',
        mimeType: 'text/plain',
        sizeBytes: fullText.length,
        url,
        inlineData: null,
      },
      ts: Date.now(),
    })

    const fetchMock = vi.mocked(globalThis.fetch)
    fetchMock.mockResolvedValue({
      ok: true,
      arrayBuffer: async () => new TextEncoder().encode(fullText).buffer,
    } as Response)

    render(
      <ClipboardItem
        index={1}
        type="text"
        time="just now"
        content={{ display_text: preview, has_detail: true, size: fullText.length }}
        entryId="entry-1"
      />
    )

    await userEvent.click(screen.getByText(/Expand|展开/))

    await waitFor(() => {
      expect(requestMock).toHaveBeenCalledWith('/clipboard/entries/entry-1/resource')
      expect(fetchMock).toHaveBeenCalledWith(
        'http://127.0.0.1:12345/clipboard/blobs/blob-1?auth=Session+test'
      )
    })

    expect(await screen.findByText(fullText)).toBeInTheDocument()
  })

  it('loads blob-backed images through authenticated daemon URLs', async () => {
    requestMock.mockResolvedValue({
      data: {
        blobId: 'image-1',
        mimeType: 'image/png',
        sizeBytes: 123,
        url: '/clipboard/blobs/image-1',
        inlineData: null,
      },
      ts: Date.now(),
    })

    render(
      <ClipboardItem
        index={2}
        type="image"
        time="just now"
        content={{ thumbnail: null, size: 123, width: 10, height: 10 }}
        entryId="entry-2"
      />
    )

    await waitFor(() => {
      expect(requestMock).toHaveBeenCalledWith('/clipboard/entries/entry-2/resource')
    })

    const image = await screen.findByAltText(/Clipboard Image|剪贴板图片/)
    expect(image).toHaveAttribute(
      'src',
      'http://127.0.0.1:12345/clipboard/blobs/image-1?auth=Session+test'
    )
  })

  it('keeps inline data image URLs as data URLs instead of prefixing daemon base URL', async () => {
    requestMock.mockResolvedValue({
      data: {
        blobId: null,
        mimeType: 'image/png',
        sizeBytes: 123,
        url: null,
        inlineData: 'iVBORw0KGgo=',
      },
      ts: Date.now(),
    })

    render(
      <ClipboardItem
        index={3}
        type="image"
        time="just now"
        content={{ thumbnail: null, size: 123, width: 10, height: 10 }}
        entryId="entry-3"
      />
    )

    const image = await screen.findByAltText(/Clipboard Image|剪贴板图片/)
    expect(image).toHaveAttribute('src', 'data:image/png;base64,iVBORw0KGgo=')
    expect(blobUrlMock).not.toHaveBeenCalledWith('data:image/png;base64,iVBORw0KGgo=')
  })
})
