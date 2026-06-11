import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { daemonClient } from '@/api/daemon/client'
import { getClipboardEntryResource } from '@/api/generated/sdk.gen'
import ClipboardItem from '@/components/clipboard/ClipboardItem'

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    // Replay the happy path: callSdk unwraps the SDK's outer { data } to the envelope.
    callSdk: vi.fn((call: () => Promise<{ data: unknown }>) => call().then(r => r.data)),
    // 复刻 callEnveloped 快乐路径：连拆 SDK { data } 与 { data, ts } 信封。
    callEnveloped: vi.fn((call: () => Promise<{ data: { data: unknown } }>) =>
      call().then(r => r.data.data)
    ),
    blobUrl: vi.fn((path: string) => `http://127.0.0.1:12345${path}?auth=Session+test`),
  },
}))

vi.mock('@/api/generated/sdk.gen', () => ({
  getClipboardEntryResource: vi.fn(),
}))

const resourceMock = vi.mocked(getClipboardEntryResource)
const blobUrlMock = vi.mocked(daemonClient.blobUrl)

// Resolve the SDK fn to { data: envelope } where envelope = { data: payload, ts }.
function mockResource(payload: unknown) {
  resourceMock.mockResolvedValue({ data: { data: payload, ts: Date.now() } } as never)
}

describe('ClipboardItem', () => {
  beforeEach(() => {
    resourceMock.mockReset()
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

    mockResource({
      blobId: 'blob-1',
      mimeType: 'text/plain',
      sizeBytes: fullText.length,
      url,
      inlineData: null,
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
      expect(resourceMock).toHaveBeenCalledWith({ path: { id: 'entry-1' }, throwOnError: true })
      expect(fetchMock).toHaveBeenCalledWith(
        'http://127.0.0.1:12345/clipboard/blobs/blob-1?auth=Session+test'
      )
    })

    expect(await screen.findByText(fullText)).toBeInTheDocument()
  })

  it('loads blob-backed images through authenticated daemon URLs on demand', async () => {
    mockResource({
      blobId: 'image-1',
      mimeType: 'image/png',
      sizeBytes: 123,
      url: '/clipboard/blobs/image-1',
      inlineData: null,
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

    expect(resourceMock).not.toHaveBeenCalled()

    await userEvent.click(screen.getByText(/Expand|展开/))

    await waitFor(() => {
      expect(resourceMock).toHaveBeenCalledWith({ path: { id: 'entry-2' }, throwOnError: true })
    })

    const image = await screen.findByAltText(/Clipboard Image|剪贴板图片/)
    expect(image).toHaveAttribute(
      'src',
      'http://127.0.0.1:12345/clipboard/blobs/image-1?auth=Session+test'
    )
  })

  it('keeps inline data image URLs as data URLs instead of prefixing daemon base URL', async () => {
    mockResource({
      blobId: null,
      mimeType: 'image/png',
      sizeBytes: 123,
      url: null,
      inlineData: 'iVBORw0KGgo=',
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

    expect(resourceMock).not.toHaveBeenCalled()

    await userEvent.click(screen.getByText(/Expand|展开/))

    const image = await screen.findByAltText(/Clipboard Image|剪贴板图片/)
    expect(image).toHaveAttribute('src', 'data:image/png;base64,iVBORw0KGgo=')
    expect(blobUrlMock).not.toHaveBeenCalledWith('data:image/png;base64,iVBORw0KGgo=')
  })
})
