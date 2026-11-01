import { beforeEach, describe, expect, it, vi } from 'vitest'
import { clipboardPreviewCache } from '@/lib/clipboard-preview-cache'
import { loadClipboardPreview } from '@/lib/clipboard-preview-loader'

vi.mock('@/lib/clipboard-preview-loader', () => ({ loadClipboardPreview: vi.fn() }))

describe('selected clipboard preview retention', () => {
  beforeEach(() => {
    clipboardPreviewCache.clear()
    vi.mocked(loadClipboardPreview).mockImplementation(async entryId => ({
      entryId,
      contentType: 'text',
      sizeBytes: 1,
    }))
  })
  it('retains the selected preview and pending fetch alongside the bounded list', async () => {
    await clipboardPreviewCache.get('selected')
    await clipboardPreviewCache.get('discarded')
    const pending = clipboardPreviewCache.get('pending-selected')
    clipboardPreviewCache.retain(new Set(['window']), 'pending-selected')
    await pending
    expect(clipboardPreviewCache.peek('selected')).toBeNull()
    expect(clipboardPreviewCache.peek('discarded')).toBeNull()
    expect(clipboardPreviewCache.peek('pending-selected')?.entryId).toBe('pending-selected')
    clipboardPreviewCache.retain(new Set(['window']), 'pending-selected')
    expect(clipboardPreviewCache.peek('pending-selected')).not.toBeNull()
    clipboardPreviewCache.retain(new Set(['window']))
    expect(clipboardPreviewCache.peek('pending-selected')).toBeNull()
  })
})
