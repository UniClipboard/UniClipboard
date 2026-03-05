import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import ClipboardItem from '@/components/clipboard/ClipboardItem'
import { invokeWithTrace } from '@/lib/tauri-command'

vi.mock('@/lib/tauri-command', () => ({
  invokeWithTrace: vi.fn(),
}))

// resolveUcUrl no longer depends on @tauri-apps/api/core.
// In test env (non-Windows userAgent), it produces uc://localhost/{path} format.

const invokeMock = vi.mocked(invokeWithTrace)

describe('ClipboardItem', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    globalThis.fetch = vi.fn()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('expands by fetching resource bytes and decoding text', async () => {
    const preview = 'x'.repeat(260)
    const fullText = 'full content'
    const url = 'uc://blob/blob-1'

    invokeMock.mockResolvedValue({
      blob_id: 'blob-1',
      mime_type: 'text/plain',
      size_bytes: fullText.length,
      url,
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
      expect(invokeMock).toHaveBeenCalledWith('get_clipboard_entry_resource', {
        entryId: 'entry-1',
      })
      // resolveUcUrl converts uc://blob/blob-1 → uc://localhost/blob/blob-1
      expect(fetchMock).toHaveBeenCalledWith('uc://localhost/blob/blob-1')
    })

    expect(await screen.findByText(fullText)).toBeInTheDocument()
  })

  it('expands image by loading resource url', async () => {
    const url = 'uc://blob/image-1'
    const thumbnail = 'thumb://image-1'

    invokeMock.mockResolvedValue({
      blob_id: 'image-1',
      mime_type: 'image/png',
      size_bytes: 123,
      url,
    })

    render(
      <ClipboardItem
        index={2}
        type="image"
        time="just now"
        content={{ thumbnail, size: 123, width: 10, height: 10 }}
        entryId="entry-2"
      />
    )

    const image = screen.getByAltText(/Clipboard Image|剪贴板图片/)
    expect(image).toHaveAttribute('src', thumbnail)

    await userEvent.click(screen.getByText(/Expand|展开/))

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('get_clipboard_entry_resource', {
        entryId: 'entry-2',
      })
    })

    // resolveUcUrl converts uc://blob/image-1 → uc://localhost/blob/image-1
    expect(image).toHaveAttribute('src', 'uc://localhost/blob/image-1')
  })
})
