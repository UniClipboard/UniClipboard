import { describe, expect, it } from 'vitest'
import { resolveUcUrl } from '@/lib/protocol'

describe('resolveUcUrl', () => {
  it('resolves uc://thumbnail/rep-1 to platform-correct URL (non-Windows)', () => {
    const result = resolveUcUrl('uc://thumbnail/rep-1')
    // In test env (non-Windows), uses uc://localhost/ format
    expect(result).toBe('uc://localhost/thumbnail/rep-1')
  })

  it('resolves uc://blob/blob-1 to platform-correct URL', () => {
    const result = resolveUcUrl('uc://blob/blob-1')
    expect(result).toBe('uc://localhost/blob/blob-1')
  })

  it('returns non-uc:// URLs unchanged', () => {
    const result = resolveUcUrl('https://example.com')
    expect(result).toBe('https://example.com')
  })

  it('returns empty string unchanged', () => {
    const result = resolveUcUrl('')
    expect(result).toBe('')
  })

  it('preserves path segments without encoding slashes', () => {
    const result = resolveUcUrl('uc://thumbnail/2614aa37-e227-4bba-b084-ed1c2ac59c85')
    expect(result).toBe('uc://localhost/thumbnail/2614aa37-e227-4bba-b084-ed1c2ac59c85')
    // Must NOT contain %2F
    expect(result).not.toContain('%2F')
  })
})
