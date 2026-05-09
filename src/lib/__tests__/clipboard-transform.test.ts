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

  it('propagates payloadState=Lost so the row can render the unavailable state', () => {
    const item = transformDaemonDtoToItemResponse({
      id: 'entry-lost',
      preview: 'file:///tmp/gone.png',
      hasDetail: false,
      sizeBytes: 92,
      capturedAt: 1,
      contentType: 'text/uri-list',
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
      payloadState: 'Lost',
    })

    expect(item.payload_state).toBe('Lost')
  })

  it('defaults payload_state to null when daemon omits the field', () => {
    const item = transformDaemonDtoToItemResponse({
      id: 'entry-healthy',
      preview: 'hello',
      hasDetail: false,
      sizeBytes: 5,
      capturedAt: 1,
      contentType: 'text/plain',
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
    })

    expect(item.payload_state).toBeNull()
  })

  it('hydrates failed file_transfer_status from daemon DTO', () => {
    const item = transformDaemonDtoToItemResponse({
      id: 'file-entry-1',
      preview: 'file:///tmp/test.txt',
      hasDetail: false,
      sizeBytes: 100,
      capturedAt: 1000,
      contentType: 'text/uri-list',
      thumbnailUrl: null,
      isEncrypted: false,
      isFavorited: false,
      updatedAt: 1000,
      activeTime: 0,
      fileTransferStatus: 'failed',
      fileTransferReason: 'timeout after 60s',
      linkUrls: null,
      linkDomains: null,
      fileSizes: null,
    })

    expect(item.file_transfer_status).toBe('failed')
    expect(item.file_transfer_reason).toBe('timeout after 60s')
  })

  it('defaults file_transfer_status to null for non-file entries', () => {
    const item = transformDaemonDtoToItemResponse({
      id: 'text-entry-1',
      preview: 'hello world',
      hasDetail: false,
      sizeBytes: 11,
      capturedAt: 3000,
      contentType: 'text/plain',
      thumbnailUrl: null,
      isEncrypted: false,
      isFavorited: false,
      updatedAt: 3000,
      activeTime: 0,
      fileTransferStatus: null,
      fileTransferReason: null,
      linkUrls: null,
      linkDomains: null,
      fileSizes: null,
    })

    expect(item.file_transfer_status).toBeNull()
  })
})
