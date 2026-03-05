import { convertFileSrc } from '@tauri-apps/api/core'
import { describe, expect, it, vi } from 'vitest'
import { resolveUcUrl } from '@/lib/protocol'

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: vi.fn((path: string, protocol: string) => `${protocol}://localhost/${path}`),
}))

const convertMock = vi.mocked(convertFileSrc)

describe('resolveUcUrl', () => {
  beforeEach(() => {
    convertMock.mockClear()
  })

  it('resolves uc://thumbnail/rep-1 via convertFileSrc', () => {
    convertMock.mockReturnValue('uc://localhost/thumbnail/rep-1')
    const result = resolveUcUrl('uc://thumbnail/rep-1')
    expect(convertMock).toHaveBeenCalledWith('thumbnail/rep-1', 'uc')
    expect(result).toBe('uc://localhost/thumbnail/rep-1')
  })

  it('resolves uc://blob/blob-1 via convertFileSrc', () => {
    convertMock.mockReturnValue('uc://localhost/blob/blob-1')
    const result = resolveUcUrl('uc://blob/blob-1')
    expect(convertMock).toHaveBeenCalledWith('blob/blob-1', 'uc')
    expect(result).toBe('uc://localhost/blob/blob-1')
  })

  it('returns non-uc:// URLs unchanged', () => {
    const result = resolveUcUrl('https://example.com')
    expect(convertMock).not.toHaveBeenCalled()
    expect(result).toBe('https://example.com')
  })

  it('returns empty string unchanged', () => {
    const result = resolveUcUrl('')
    expect(convertMock).not.toHaveBeenCalled()
    expect(result).toBe('')
  })
})
