import { deflateSync } from 'node:zlib'

// A 160 x 100 RGB gradient PNG, so the thumbnail and the detail show a recognisable picture.
export function gradientPng(width = 160, height = 100) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    return c >>> 0
  })
  const crc = bytes => {
    let c = 0xffffffff
    for (const b of bytes) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8)
    return (c ^ 0xffffffff) >>> 0
  }
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
    const out = Buffer.alloc(body.length + 8)
    out.writeUInt32BE(data.length, 0)
    body.copy(out, 4)
    out.writeUInt32BE(crc(body), body.length + 4)
    return out
  }
  const header = Buffer.alloc(13)
  header.writeUInt32BE(width, 0)
  header.writeUInt32BE(height, 4)
  header[8] = 8 // bit depth
  header[9] = 2 // RGB
  const raw = Buffer.alloc((width * 3 + 1) * height)
  for (let y = 0; y < height; y++) {
    const row = y * (width * 3 + 1)
    for (let x = 0; x < width; x++) {
      raw[row + 1 + x * 3] = 40 + Math.round((x / width) * 180)
      raw[row + 2 + x * 3] = 90 + Math.round((y / height) * 120)
      raw[row + 3 + x * 3] = 200 - Math.round((x / width) * 120)
    }
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ])
}

export const HISTORY_TEXTS = [
  'Q4 roadmap: ship LAN-only mode, then per-device delivery receipts',
  'https://github.com/UniClipboard/UniClipboard',
  'git rebase -i HEAD~3',
  'docker compose -f compose.prod.yml \\\n  --env-file .env.prod \\\n  up -d --build --remove-orphans',
  'Meeting notes\n- decide the Windows title bar layout\n- review keyboard focus order',
]
