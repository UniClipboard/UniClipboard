/**
 * 跨语言契约测试 —— TS 端 `mobileSyncConnectUri.ts` 必须与 Rust 端
 * `src-tauri/.../mobile_sync/connect_uri.rs` 字节级一致。
 *
 * 核心保护: `GOLDEN_URI` 是规范 §7.1 的 happy-path 字面量, 在 Rust
 * 测试 (`connect_uri.rs:282`) 和这里完全相同。任一侧编码漂移都会让
 * `build_emits_golden_uri` 立刻失败 —— 跨语言 contract test 的目的。
 *
 * 6 个负例对应规范 §7.2 + Rust `parse_rejects_*` 一一镜像。
 */

import { describe, expect, it } from 'vitest'
import {
  buildConnectUri,
  ConnectUriError,
  parseConnectUri,
  URI_MAX_LEN,
  type ConnectUriOther,
} from '../mobileSyncConnectUri'

/**
 * Golden vector — 与 Rust `connect_uri.rs:282` 字面量字节相同。
 * 任何修改都必须同步更新 Rust 端 + 规范文档 §7.1。
 */
const GOLDEN_URI =
  'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ'

function goldenOther(): ConnectUriOther {
  return {
    label: 'Test',
    did: 'did_0123abcd',
    proto: 'syncclipboard',
  }
}

describe('mobileSyncConnectUri / build (happy + byte stability)', () => {
  it('emits the golden URI byte-for-byte (matches Rust)', () => {
    const uri = buildConnectUri(
      'http://192.168.1.5:42720',
      'mobile_aabbccdd',
      'AbCdEfGhIjKlMnOpQrSt',
      goldenOther()
    )
    expect(uri).toBe(GOLDEN_URI)
  })

  it("drops empty other map so JSON has no 'o' field", () => {
    // 跟 Rust `build_drops_empty_other_map` 同语义: 不应出现 "o":{}, 否则
    // base64 字节会漂移。
    const uri = buildConnectUri('http://a.b', 'user', 'pass')
    const p = uri.split('p=')[1]
    const json = new TextDecoder().decode(base64UrlToBytes(p))
    expect(json.includes('"o"')).toBe(false)
  })

  it('orders `o` keys lexicographically regardless of insertion order', () => {
    // 故意按非字典序传入 —— 编码侧必须强制 did → install → label → proto。
    const uri = buildConnectUri('http://a.b', 'user', 'pwd', {
      proto: 'syncclipboard',
      label: 'L',
      did: 'D',
      install: 'I',
    })
    const p = uri.split('p=')[1]
    const json = new TextDecoder().decode(base64UrlToBytes(p))
    const didPos = json.indexOf('"did"')
    const installPos = json.indexOf('"install"')
    const labelPos = json.indexOf('"label"')
    const protoPos = json.indexOf('"proto"')
    expect(didPos).toBeGreaterThan(-1)
    expect(installPos).toBeGreaterThan(didPos)
    expect(labelPos).toBeGreaterThan(installPos)
    expect(protoPos).toBeGreaterThan(labelPos)
  })
})

describe('mobileSyncConnectUri / build (negative)', () => {
  it('rejects empty url with MISSING_FIELD', () => {
    expect(() => buildConnectUri('', 'user', 'pwd')).toThrowError(
      expect.objectContaining({
        code: 'MISSING_FIELD',
        field: 'url',
      }) as never
    )
  })

  it('rejects empty user with MISSING_FIELD', () => {
    expect(() => buildConnectUri('http://a.b', '', 'pwd')).toThrowError(
      expect.objectContaining({
        code: 'MISSING_FIELD',
        field: 'user',
      }) as never
    )
  })

  it('rejects empty pwd with MISSING_FIELD', () => {
    expect(() => buildConnectUri('http://a.b', 'user', '')).toThrowError(
      expect.objectContaining({
        code: 'MISSING_FIELD',
        field: 'pwd',
      }) as never
    )
  })

  it('rejects non-http url with INVALID_URL', () => {
    expect(() => buildConnectUri('ftp://a.b', 'user', 'pwd')).toThrowError(
      expect.objectContaining({ code: 'INVALID_URL' }) as never
    )
  })

  it('rejects too-long URI with URI_TOO_LONG (carries len/max)', () => {
    try {
      buildConnectUri('http://a.b', 'user', 'pwd', {
        label: 'L'.repeat(1000),
      })
      throw new Error('expected URI_TOO_LONG to throw')
    } catch (err) {
      expect(err).toBeInstanceOf(ConnectUriError)
      const e = err as ConnectUriError
      expect(e.code).toBe('URI_TOO_LONG')
      expect(e.max).toBe(URI_MAX_LEN)
      expect(e.len).toBeGreaterThan(URI_MAX_LEN)
    }
  })
})

describe('mobileSyncConnectUri / parse (happy + trim)', () => {
  it('round-trips the golden URI back to original fields', () => {
    const p = parseConnectUri(GOLDEN_URI)
    expect(p.v).toBe(1)
    expect(p.url).toBe('http://192.168.1.5:42720')
    expect(p.user).toBe('mobile_aabbccdd')
    expect(p.pwd).toBe('AbCdEfGhIjKlMnOpQrSt')
    expect(p.o).toEqual({
      did: 'did_0123abcd',
      label: 'Test',
      proto: 'syncclipboard',
    })
  })

  it('trims surrounding whitespace before parsing', () => {
    const padded = `  \n${GOLDEN_URI}\t  `
    expect(() => parseConnectUri(padded)).not.toThrow()
  })
})

describe('mobileSyncConnectUri / parse (negative, mirrors Rust §7.2)', () => {
  it('§7.2 #1 — rejects wrong scheme (https://) with INVALID_SCHEME', () => {
    expect(() =>
      parseConnectUri('https://example.com/connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ')
    ).toThrowError(expect.objectContaining({ code: 'INVALID_SCHEME' }) as never)
  })

  it('rejects uniclip:// alias with INVALID_SCHEME (single-scheme decision)', () => {
    expect(() =>
      parseConnectUri('uniclip://connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ')
    ).toThrowError(expect.objectContaining({ code: 'INVALID_SCHEME' }) as never)
  })

  it('rejects wrong host with INVALID_SCHEME', () => {
    expect(() =>
      parseConnectUri('uniclipboard://other?v=1&svc=mobile-sync&p=eyJ2IjoxfQ')
    ).toThrowError(expect.objectContaining({ code: 'INVALID_SCHEME' }) as never)
  })

  it('§7.2 #2 — rejects unsupported envelope v with UNSUPPORTED_VERSION', () => {
    expect(() =>
      parseConnectUri('uniclipboard://connect?v=2&svc=mobile-sync&p=eyJ2IjoxfQ')
    ).toThrowError(expect.objectContaining({ code: 'UNSUPPORTED_VERSION' }) as never)
  })

  it('§7.2 #3 — rejects unsupported service with UNSUPPORTED_SERVICE', () => {
    expect(() => parseConnectUri('uniclipboard://connect?v=1&svc=other&p=eyJ2IjoxfQ')).toThrowError(
      expect.objectContaining({ code: 'UNSUPPORTED_SERVICE' }) as never
    )
  })

  it('§7.2 #4 — rejects malformed base64 with PAYLOAD_DECODE_FAILED', () => {
    expect(() =>
      parseConnectUri('uniclipboard://connect?v=1&svc=mobile-sync&p=not-valid-base64!@#')
    ).toThrowError(expect.objectContaining({ code: 'PAYLOAD_DECODE_FAILED' }) as never)
  })

  it('§7.2 #5 — rejects missing pwd with MISSING_FIELD(pwd)', () => {
    // base64 of {"v":1,"url":"http://a.b","user":"u"}
    const uri =
      'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vYS5iIiwidXNlciI6InUifQ'
    expect(() => parseConnectUri(uri)).toThrowError(
      expect.objectContaining({ code: 'MISSING_FIELD', field: 'pwd' }) as never
    )
  })

  it('§7.2 #6 — rejects non-http url in payload with INVALID_URL', () => {
    // base64 of {"v":1,"url":"ftp://a.b","user":"u","pwd":"p"}
    const uri =
      'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJmdHA6Ly9hLmIiLCJ1c2VyIjoidSIsInB3ZCI6InAifQ'
    expect(() => parseConnectUri(uri)).toThrowError(
      expect.objectContaining({ code: 'INVALID_URL' }) as never
    )
  })

  it('rejects missing p param with PAYLOAD_DECODE_FAILED', () => {
    expect(() => parseConnectUri('uniclipboard://connect?v=1&svc=mobile-sync')).toThrowError(
      expect.objectContaining({ code: 'PAYLOAD_DECODE_FAILED' }) as never
    )
  })

  it('rejects payload v mismatch with UNSUPPORTED_VERSION', () => {
    // base64 of {"v":2,"url":"http://a.b","user":"u","pwd":"p"} ↓
    const uri =
      'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoyLCJ1cmwiOiJodHRwOi8vYS5iIiwidXNlciI6InUiLCJwd2QiOiJwIn0'
    expect(() => parseConnectUri(uri)).toThrowError(
      expect.objectContaining({ code: 'UNSUPPORTED_VERSION' }) as never
    )
  })
})

describe('mobileSyncConnectUri / forward compat + round-trip', () => {
  it('parse ignores unknown o.* keys (forward compat)', () => {
    // base64 of {"v":1,"url":"http://a.b","user":"u","pwd":"p","o":{"future_key":"future_val","label":"L"}}
    const uri =
      'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vYS5iIiwidXNlciI6InUiLCJwd2QiOiJwIiwibyI6eyJmdXR1cmVfa2V5IjoiZnV0dXJlX3ZhbCIsImxhYmVsIjoiTCJ9fQ'
    const p = parseConnectUri(uri)
    expect(p.o.future_key).toBe('future_val')
    expect(p.o.label).toBe('L')
  })

  it('build → parse preserves Unicode label + all known o fields', () => {
    const uri = buildConnectUri('http://10.0.0.5:42720', 'alice_001', 'p@ssw0rd-with-symbols', {
      label: '我的 iPhone',
      did: 'did_xyz',
      proto: 'syncclipboard',
      install: 'shortcut-ex',
    })
    const p = parseConnectUri(uri)
    expect(p.url).toBe('http://10.0.0.5:42720')
    expect(p.user).toBe('alice_001')
    expect(p.pwd).toBe('p@ssw0rd-with-symbols')
    expect(p.o).toEqual({
      label: '我的 iPhone',
      did: 'did_xyz',
      proto: 'syncclipboard',
      install: 'shortcut-ex',
    })
  })
})

// ─── private helper for build-side byte introspection in tests ─────────
// 测试需要把 build 出的 base64url 还原成 JSON 字节, 才能断言 "o 不出现"
// 与字典序。这一份内嵌实现独立于 production 代码, 避免把 base64UrlToBytes
// 暴露成 public API。

function base64UrlToBytes(text: string): Uint8Array {
  let b64 = text.replace(/-/g, '+').replace(/_/g, '/')
  while (b64.length % 4 !== 0) b64 += '='
  const binary = atob(b64)
  const out = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i)
  return out
}
