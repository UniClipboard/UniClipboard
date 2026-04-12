import { describe, expect, it } from 'vitest'
import { transformDaemonDtoToItemResponse } from '../clipboard-transform'
import { getItemPreview } from '../clipboard-utils'

describe('clipboard-transform', () => {
  it('preserves image dimensions from daemon DTOs', () => {
    const item = transformDaemonDtoToItemResponse({
      id: 'entry-1',
      preview: 'Image (123 bytes)',
      hasDetail: false,
      sizeBytes: 123,
      capturedAt: 1,
      contentType: 'image/png',
      thumbnailUrl: null,
      isEncrypted: false,
      isFavorited: false,
      updatedAt: 1,
      activeTime: 1,
      fileTransferStatus: null,
      fileTransferReason: null,
      linkUrls: null,
      linkDomains: null,
      fileSizes: null,
      imageWidth: 1920,
      imageHeight: 1080,
    })

    expect(item.item.image).toEqual({
      thumbnail: null,
      size: 123,
      width: 1920,
      height: 1080,
    })
    expect(getItemPreview(item)).toBe('Image | 1920×1080 | 123 B')
  })
})
