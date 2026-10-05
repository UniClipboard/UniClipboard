import { getBlobImageObjectUrl } from '@/api/daemon/blob-image-cache'

const FORMATS: Record<string, { label: string; extension: string }> = {
  'image/png': { label: 'PNG', extension: 'png' },
  'image/jpeg': { label: 'JPEG', extension: 'jpg' },
  'image/gif': { label: 'GIF', extension: 'gif' },
  'image/webp': { label: 'WebP', extension: 'webp' },
  'image/bmp': { label: 'BMP', extension: 'bmp' },
  'image/tiff': { label: 'TIFF', extension: 'tiff' },
  'image/svg+xml': { label: 'SVG', extension: 'svg' },
  'image/avif': { label: 'AVIF', extension: 'avif' },
  'image/heic': { label: 'HEIC', extension: 'heic' },
}

/** Short format name for a MIME type (`image/png` → `PNG`), or `null` when unknown. */
export function imageFormatLabel(mime: string | null | undefined): string | null {
  return (mime && FORMATS[mime.toLowerCase()]?.label) || null
}

/** File name for an exported image, e.g. `Clipboard image.png`. */
export function imageFileName(baseName: string, mime: string | null | undefined): string {
  const extension = (mime && FORMATS[mime.toLowerCase()]?.extension) || 'png'
  return `${baseName}.${extension}`
}

/**
 * Fetch the image behind a preview descriptor (`data:` URL or daemon blob
 * path). Daemon blobs go through the shared object-URL cache, so an image that
 * is already on screen is not downloaded twice.
 */
export async function loadImageBlob(descriptor: string): Promise<Blob> {
  const url = descriptor.startsWith('data:') ? descriptor : await getBlobImageObjectUrl(descriptor)
  if (!url) throw new Error('image is unavailable')
  const response = await fetch(url)
  return response.blob()
}
