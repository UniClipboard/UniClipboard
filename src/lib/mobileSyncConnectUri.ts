/**
 * `mobile_sync::connect_uri` 的 TS 镜像实现 —— `uniclipboard://connect` 深链
 * 协议 v1 的编解码纯函数。
 *
 * # 为什么需要这个文件
 *
 * - 后端在 `src-tauri/.../usecases/mobile_sync/connect_uri.rs` 生成 QR 内容,
 *   `RegisterMobileDeviceResult.connectUri` DTO 字段把它原样透传给前端。
 * - 前端为何还要一份解码器?
 *   1. 自检: 后端给的 connect URI 是否能在浏览器侧 round-trip 出三栏 ——
 *      `MobileSyncCredentialModal` 阶段 3B 在渲染前会跑一次 parse, 失败
 *      给出"扫码会失败, 请重新生成设备"提示。
 *   2. 跨语言契约: 与 Rust `connect_uri.rs:tests::GOLDEN_URI` 共享同一份
 *      golden vector, 任一侧字节漂移会立刻被测试套发现。
 *   3. 编码器对外暴露给未来"用户在前端粘贴连接字符串"的反向用法(暂未
 *      接入), 不强行下沉到 Tauri 层。
 *
 * # 字节级一致性约束 (与 Rust 端镜像)
 *
 * 必须严格执行才能让 build 出的字符串在两端字节相等:
 *
 * 1. JSON 字段顺序固定 `v / url / user / pwd / o` —— 手动构造对象, 不依赖
 *    `JSON.stringify` 的隐式键顺序。
 * 2. `o` 内部键字典序 —— `Object.keys(o).sort()` 后逐项插入。
 * 3. JSON 不带空白 —— `JSON.stringify` 默认即 minify。
 * 4. 空 `o` 不序列化 —— 跳过 `o` 字段, 避免 `"o":{}` 让 base64 漂移。
 * 5. base64url-no-pad —— 标准 base64 后做 `+→-`, `/→_`, 去 `=` padding。
 * 6. UTF-8 编码 —— `TextEncoder` → 字节数组 → base64, 与 Rust
 *    `json.as_bytes()` 同语义。
 *
 * # 规范
 *
 * 单一真相在 `docs/architecture/mobile-sync-connect-uri.md`。修改本文件前
 * 必须先同步更新规范文档 + golden vector + Rust 实现。
 */

// ─── public types ───────────────────────────────────────────────────────

/**
 * 解析成功后的 payload 结构 —— 与 Rust `ConnectPayload` 字段对齐。
 *
 * - `v` 始终为 1(payload schema 版本; v2 已被本侧拒绝, 不会出现在这里)。
 * - `o` 是宽松 KV: 解析侧保留任意键(前向兼容), 调用方按需读取已知键。
 */
export interface ConnectPayload {
  v: 1
  url: string
  user: string
  pwd: string
  o: Record<string, string>
}

/**
 * build 侧 `o` 字段白名单 —— 类型层约束防误塞敏感字段(规范 §5.2)。
 * 新增字段必须先更新规范文档 §3.2 + Rust `ConnectUriOther`, 再加这里。
 */
export interface ConnectUriOther {
  /** 设备显示标签, 用于客户端 UI。 */
  label?: string
  /** 服务端 device_id, 用于日志关联。 */
  did?: string
  /** 协议族提示, v1 仅 `"syncclipboard"`。 */
  proto?: string
  /** iOS Shortcut 模板提示, v1 暂不使用。 */
  install?: string
}

/**
 * 错误码与规范 §4.2 表 + Rust `ConnectUriError` 一一对应。
 *
 * 失败语义讨论见 Rust 端模块注释; 错误码用大写下划线形式与规范文档对齐,
 * 同时方便前端 i18n 文案 key 复用("CONNECT_URI_INVALID_SCHEME" 等)。
 */
export type ConnectUriErrorCode =
  | 'INVALID_SCHEME'
  | 'UNSUPPORTED_VERSION'
  | 'UNSUPPORTED_SERVICE'
  | 'PAYLOAD_DECODE_FAILED'
  | 'MISSING_FIELD'
  | 'INVALID_URL'
  | 'URI_TOO_LONG'

/**
 * 跨调用方失败语义 —— 抛错而非 Result<T, E>, 因为 JS 生态期望异常通道,
 * 也让上层 try/catch 自然组合 i18n 错误展示。
 */
export class ConnectUriError extends Error {
  readonly code: ConnectUriErrorCode
  /** `MISSING_FIELD` 时携带 url / user / pwd 三选一。 */
  readonly field?: 'url' | 'user' | 'pwd'
  /** `URI_TOO_LONG` 时携带 len/max, 供文案展示"超出 X / Y 字符"。 */
  readonly len?: number
  readonly max?: number
  /** `PAYLOAD_DECODE_FAILED` 时携带底层 base64 / JSON 错误描述。 */
  readonly detail?: string

  constructor(
    code: ConnectUriErrorCode,
    extra: {
      field?: 'url' | 'user' | 'pwd'
      len?: number
      max?: number
      detail?: string
    } = {}
  ) {
    super(formatMessage(code, extra))
    this.name = 'ConnectUriError'
    this.code = code
    this.field = extra.field
    this.len = extra.len
    this.max = extra.max
    this.detail = extra.detail
  }
}

function formatMessage(
  code: ConnectUriErrorCode,
  extra: { field?: string; len?: number; max?: number; detail?: string }
): string {
  switch (code) {
    case 'INVALID_SCHEME':
      return 'invalid scheme or host (must be uniclipboard://connect)'
    case 'UNSUPPORTED_VERSION':
      return 'unsupported version (only v=1 is supported)'
    case 'UNSUPPORTED_SERVICE':
      return 'unsupported service (only svc=mobile-sync is supported)'
    case 'PAYLOAD_DECODE_FAILED':
      return `payload decode failed: ${extra.detail ?? 'unknown'}`
    case 'MISSING_FIELD':
      return `required field missing or empty: ${extra.field ?? 'unknown'}`
    case 'INVALID_URL':
      return 'invalid url: must start with http:// or https://'
    case 'URI_TOO_LONG':
      return `uri too long (${extra.len ?? '?'} chars, max ${extra.max ?? '?'})`
  }
}

// ─── constants ──────────────────────────────────────────────────────────

const SCHEME = 'uniclipboard:'
const HOST = 'connect'
const ENVELOPE_VERSION = '1'
const SERVICE = 'mobile-sync'
const PAYLOAD_VERSION = 1
/** 规范 §2 URI 长度上限(易扫描 + 防 `o` 滥用)。与 Rust 端常量一致。 */
export const URI_MAX_LEN = 800

// ─── build ──────────────────────────────────────────────────────────────

/**
 * 把凭据 + 元数据编码成 `uniclipboard://connect?v=1&svc=mobile-sync&p=<…>`。
 *
 * 与 Rust [`build_mobile_sync_connect_uri`] 字节级镜像 —— 任何漂移会在
 * 跨语言 golden vector 测试中立刻失败。
 *
 * 失败语义见 `ConnectUriError`:
 * - `MISSING_FIELD` 当 url/user/pwd 为空字符串
 * - `INVALID_URL` 当 url 不以 `http://` 或 `https://` 开头
 * - `URI_TOO_LONG` 当结果超过 `URI_MAX_LEN` 字符
 *
 * [`build_mobile_sync_connect_uri`]: ../../src-tauri/crates/uc-application/src/usecases/mobile_sync/connect_uri.rs
 */
export function buildConnectUri(
  baseUrl: string,
  username: string,
  password: string,
  other: ConnectUriOther = {}
): string {
  if (baseUrl === '') {
    throw new ConnectUriError('MISSING_FIELD', { field: 'url' })
  }
  if (username === '') {
    throw new ConnectUriError('MISSING_FIELD', { field: 'user' })
  }
  if (password === '') {
    throw new ConnectUriError('MISSING_FIELD', { field: 'pwd' })
  }
  if (!baseUrl.startsWith('http://') && !baseUrl.startsWith('https://')) {
    throw new ConnectUriError('INVALID_URL')
  }

  // 显式按 v/url/user/pwd/o 顺序构造对象, 不依赖 JSON.stringify 的隐式
  // 键顺序。`o` 内部键再单独按字典序插入(规范 §3.1 字节稳定性约定)。
  const payload: Record<string, unknown> = {
    v: PAYLOAD_VERSION,
    url: baseUrl,
    user: username,
    pwd: password,
  }

  const o = buildOtherMap(other)
  if (Object.keys(o).length > 0) {
    payload.o = o
  }

  const json = JSON.stringify(payload)
  const p = bytesToBase64Url(utf8Encode(json))
  const uri = `uniclipboard://${HOST}?v=${ENVELOPE_VERSION}&svc=${SERVICE}&p=${p}`

  if (uri.length > URI_MAX_LEN) {
    throw new ConnectUriError('URI_TOO_LONG', { len: uri.length, max: URI_MAX_LEN })
  }
  return uri
}

/**
 * 把 `ConnectUriOther` 转成键字典序的纯对象 —— BTreeMap 在 TS 端的等价物。
 *
 * 只保留非空 string 字段, 避免 `undefined` / `null` 进 JSON。
 */
function buildOtherMap(other: ConnectUriOther): Record<string, string> {
  const entries: Array<[string, string]> = []
  if (typeof other.did === 'string') entries.push(['did', other.did])
  if (typeof other.install === 'string') entries.push(['install', other.install])
  if (typeof other.label === 'string') entries.push(['label', other.label])
  if (typeof other.proto === 'string') entries.push(['proto', other.proto])

  // 显式按 key 字典序排序 + 用普通对象按插入顺序保留 ——
  // JSON.stringify 在 V8 / JSC 实现里按"插入顺序"输出字符串键, 与
  // Rust BTreeMap 字典序一致。
  entries.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
  const out: Record<string, string> = {}
  for (const [k, v] of entries) out[k] = v
  return out
}

// ─── parse ──────────────────────────────────────────────────────────────

/**
 * 把 QR 文本反向解码出 payload。错误码与规范 §4.2 一一对应。
 *
 * 与 Rust [`parse_mobile_sync_connect_uri`] 镜像 —— round-trip 字段一致。
 *
 * 不负责:
 * - 不做 url 可达性探测
 * - 不持久化任何字段
 * - 不修剪 pwd 前后空白(规范 §3.1: pwd 任何字节都合法)
 *
 * [`parse_mobile_sync_connect_uri`]: ../../src-tauri/crates/uc-application/src/usecases/mobile_sync/connect_uri.rs
 */
export function parseConnectUri(qrText: string): ConnectPayload {
  const raw = qrText.trim()

  let url: URL
  try {
    url = new URL(raw)
  } catch {
    throw new ConnectUriError('INVALID_SCHEME')
  }
  if (url.protocol !== SCHEME) {
    throw new ConnectUriError('INVALID_SCHEME')
  }
  if (url.hostname !== HOST) {
    throw new ConnectUriError('INVALID_SCHEME')
  }

  const params = url.searchParams
  const envelopeV = params.get('v')
  const svc = params.get('svc')
  const p = params.get('p')

  if (envelopeV !== ENVELOPE_VERSION) {
    throw new ConnectUriError('UNSUPPORTED_VERSION')
  }
  if (svc !== SERVICE) {
    throw new ConnectUriError('UNSUPPORTED_SERVICE')
  }
  if (p == null || p === '') {
    throw new ConnectUriError('PAYLOAD_DECODE_FAILED', { detail: 'p missing or empty' })
  }

  let jsonText: string
  try {
    jsonText = utf8Decode(base64UrlToBytes(p))
  } catch (err) {
    throw new ConnectUriError('PAYLOAD_DECODE_FAILED', {
      detail: `base64url: ${(err as Error).message}`,
    })
  }

  let raw_payload: unknown
  try {
    raw_payload = JSON.parse(jsonText)
  } catch (err) {
    throw new ConnectUriError('PAYLOAD_DECODE_FAILED', {
      detail: `json: ${(err as Error).message}`,
    })
  }

  const payload = coercePayload(raw_payload)

  if (payload.v !== PAYLOAD_VERSION) {
    throw new ConnectUriError('UNSUPPORTED_VERSION')
  }
  if (payload.url === '') {
    throw new ConnectUriError('MISSING_FIELD', { field: 'url' })
  }
  if (payload.user === '') {
    throw new ConnectUriError('MISSING_FIELD', { field: 'user' })
  }
  if (payload.pwd === '') {
    throw new ConnectUriError('MISSING_FIELD', { field: 'pwd' })
  }
  if (!payload.url.startsWith('http://') && !payload.url.startsWith('https://')) {
    throw new ConnectUriError('INVALID_URL')
  }

  return payload
}

/**
 * 把 `JSON.parse` 出来的 unknown 收敛成 `ConnectPayload`, 容忍字段缺失
 * (用空字符串兜底, 与 Rust 端 `#[serde(default)]` 同语义), 让后续 missing
 * field 检查统一在一处做。
 *
 * 未知 `o.*` 键宽松保留(规范 §3.2 ignore-unknown), 但仅当其值是 string;
 * 非 string 的 o 字段被静默丢弃, 避免类型污染调用方。
 */
function coercePayload(raw: unknown): ConnectPayload {
  if (typeof raw !== 'object' || raw === null) {
    throw new ConnectUriError('PAYLOAD_DECODE_FAILED', {
      detail: 'json: payload is not an object',
    })
  }
  const obj = raw as Record<string, unknown>
  const v = obj.v
  if (typeof v !== 'number' || !Number.isInteger(v)) {
    // v 缺失/非整数: 走 UNSUPPORTED_VERSION 而非 PAYLOAD_DECODE_FAILED ——
    // 与 Rust 端 `if payload.v != PAYLOAD_VERSION` 行为一致(serde 那侧
    // 会先反序列化为 u32 失败,我们这边显式 narrow 类型)。
    throw new ConnectUriError('UNSUPPORTED_VERSION')
  }

  const o: Record<string, string> = {}
  if (typeof obj.o === 'object' && obj.o !== null) {
    for (const [k, val] of Object.entries(obj.o as Record<string, unknown>)) {
      if (typeof val === 'string') o[k] = val
    }
  }

  return {
    v: v as 1,
    url: typeof obj.url === 'string' ? obj.url : '',
    user: typeof obj.user === 'string' ? obj.user : '',
    pwd: typeof obj.pwd === 'string' ? obj.pwd : '',
    o,
  }
}

// ─── base64url + UTF-8 helpers ─────────────────────────────────────────

/**
 * UTF-8 编码: `string → Uint8Array`。
 *
 * 浏览器 / Node / Bun 都内置 TextEncoder, 无需引外部依赖。
 */
function utf8Encode(s: string): Uint8Array {
  return new TextEncoder().encode(s)
}

/** UTF-8 解码: `Uint8Array → string`, 严格(BOM 不剥, 损坏字节报错)。 */
function utf8Decode(bytes: Uint8Array): string {
  return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
}

/**
 * `Uint8Array → base64url-no-pad`。
 *
 * `btoa` 仅接受 latin-1 (字符码 0-255) 的 "binary string", 因此先把
 * 字节逐位映射为 charCode 一致的字符, 再做标准 base64, 最后字符替换:
 * `+→-`, `/→_`, 去 `=` padding。
 */
function bytesToBase64Url(bytes: Uint8Array): string {
  // 大数组直接 String.fromCharCode(...bytes) 会触发栈溢出(参数个数有
  // 平台上限), 用 chunked 拼接更稳健。connect URI 实际 ≤ 800 字符,
  // 远不到触发点, 但保留稳健性。
  let binary = ''
  const CHUNK = 0x8000
  for (let i = 0; i < bytes.length; i += CHUNK) {
    const slice = bytes.subarray(i, Math.min(i + CHUNK, bytes.length))
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    binary += String.fromCharCode(...(slice as unknown as any))
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

/**
 * `base64url-no-pad → Uint8Array`, 反向操作。
 *
 * 先做字符替换还原标准 base64, 补 `=` padding(`atob` 要求长度是 4 的倍数),
 * 再 `atob` 拿 binary string, 最后逐字符 charCodeAt 还原字节。
 *
 * 非 base64 字符 / padding 错乱时 `atob` 抛 `InvalidCharacterError`,
 * 调用方应翻译为 `PAYLOAD_DECODE_FAILED`。
 */
function base64UrlToBytes(text: string): Uint8Array {
  let b64 = text.replace(/-/g, '+').replace(/_/g, '/')
  // base64 长度必须是 4 的倍数 —— `atob` 在这点上比 Rust URL_SAFE_NO_PAD
  // 严格, 我们手动补 `=`。
  while (b64.length % 4 !== 0) b64 += '='
  const binary = atob(b64)
  const out = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i)
  return out
}
