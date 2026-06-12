/**
 * uniclipboard://connect URI 解析器
 *
 * 协议规范：docs (移动端扫码接入协议)
 * 任何在客户端处理本 URI 的代码路径都必须遵守 spec 的安全约束：
 * - 不得把完整 URI / payload / pwd 写入日志、analytics、crash 报告
 * - 本模块内部不调用任何 logger
 */

export const CONNECT_URI_SCHEME = 'uniclipboard';
export const CONNECT_URI_HOST = 'connect';
export const CONNECT_URI_SVC = 'mobile-sync';
export const CONNECT_URI_VERSION = 1;

export type ConnectUriError =
  | 'INVALID_SCHEME'
  | 'UNSUPPORTED_VERSION'
  | 'UNSUPPORTED_SERVICE'
  | 'PAYLOAD_DECODE_FAILED'
  | 'MISSING_FIELD'
  | 'INVALID_URL';

export const CONNECT_URI_ERROR_MESSAGES: Record<ConnectUriError, string> = {
  INVALID_SCHEME: '不是 UniClipboard 的二维码。',
  UNSUPPORTED_VERSION: '请升级 App。',
  UNSUPPORTED_SERVICE: '当前版本不支持该服务。',
  PAYLOAD_DECODE_FAILED: '二维码已损坏，请重新生成。',
  MISSING_FIELD: '二维码内容不完整，请重新生成。',
  INVALID_URL: '二维码里的服务地址无效。',
};

export interface ConnectUriResult {
  url: string;
  user: string;
  pwd: string;
  label?: string;
}

export type ParseConnectUriOutcome =
  | { ok: true; value: ConnectUriResult }
  | { ok: false; error: ConnectUriError };

function base64UrlDecodeToString(input: string): string {
  // base64url → 标准 base64
  let b64 = input.replace(/-/g, '+').replace(/_/g, '/');
  const pad = b64.length % 4;
  if (pad === 2) b64 += '==';
  else if (pad === 3) b64 += '=';
  else if (pad === 1) throw new Error('invalid base64url length');

  // atob 解出 binary string；逐字节转 Uint8Array 再用 TextDecoder 还原 UTF-8
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
}

function isNonEmptyString(v: unknown): v is string {
  return typeof v === 'string' && v.length > 0;
}

/**
 * 解析 `scheme://host?query` 三段。
 *
 * 注意：故意不使用 `new URL()` —— Hermes 对 non-special scheme（uniclipboard 之类）的 URL 解析
 * 与 Node 的实现不一致，会导致 searchParams 拿不到正确值（observed in RN 0.83 + Hermes，
 * 现场症状：svc 读不到 → 误报 UNSUPPORTED_SERVICE）。手写解析跨运行时一致。
 */
function splitConnectUri(raw: string): { scheme: string; host: string; query: string } | null {
  // 匹配 scheme://host(/path 可选忽略)(?query 可选)#fragment 可选
  const m = raw.match(
    /^([a-zA-Z][a-zA-Z0-9+\-.]*):\/\/([^/?#]+)(?:\/[^?#]*)?(?:\?([^#]*))?(?:#.*)?$/
  );
  if (!m) return null;
  return { scheme: m[1], host: m[2], query: m[3] ?? '' };
}

function parseQueryParams(query: string): Map<string, string> {
  const out = new Map<string, string>();
  if (!query) return out;
  for (const part of query.split('&')) {
    if (!part) continue;
    const eq = part.indexOf('=');
    let k: string;
    let v: string;
    if (eq < 0) {
      k = part;
      v = '';
    } else {
      k = part.slice(0, eq);
      v = part.slice(eq + 1);
    }
    try {
      k = decodeURIComponent(k.replace(/\+/g, ' '));
      v = decodeURIComponent(v.replace(/\+/g, ' '));
    } catch {
      // 含非法 percent-encoding 的字段直接保留原样
    }
    // 首次出现优先（与 URLSearchParams.get 行为一致）
    if (!out.has(k)) out.set(k, v);
  }
  return out;
}

export function parseConnectUri(rawInput: string): ParseConnectUriOutcome {
  const raw = (rawInput ?? '').trim();
  if (raw.length === 0) return { ok: false, error: 'INVALID_SCHEME' };

  const split = splitConnectUri(raw);
  if (!split) return { ok: false, error: 'INVALID_SCHEME' };

  if (split.scheme.toLowerCase() !== CONNECT_URI_SCHEME) {
    return { ok: false, error: 'INVALID_SCHEME' };
  }
  if (split.host.toLowerCase() !== CONNECT_URI_HOST) {
    return { ok: false, error: 'INVALID_SCHEME' };
  }

  const params = parseQueryParams(split.query);

  const vRaw = params.get('v');
  if (vRaw === undefined || !/^-?\d+$/.test(vRaw)) {
    return { ok: false, error: 'UNSUPPORTED_VERSION' };
  }
  const v = parseInt(vRaw, 10);
  if (v !== CONNECT_URI_VERSION) {
    return { ok: false, error: 'UNSUPPORTED_VERSION' };
  }

  const svc = params.get('svc');
  if (svc !== CONNECT_URI_SVC) {
    return { ok: false, error: 'UNSUPPORTED_SERVICE' };
  }

  const p = params.get('p');
  if (!p) return { ok: false, error: 'PAYLOAD_DECODE_FAILED' };

  let jsonText: string;
  try {
    jsonText = base64UrlDecodeToString(p);
  } catch {
    return { ok: false, error: 'PAYLOAD_DECODE_FAILED' };
  }

  let payload: unknown;
  try {
    payload = JSON.parse(jsonText);
  } catch {
    return { ok: false, error: 'PAYLOAD_DECODE_FAILED' };
  }

  if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
    return { ok: false, error: 'PAYLOAD_DECODE_FAILED' };
  }
  const obj = payload as Record<string, unknown>;

  if (obj.v !== CONNECT_URI_VERSION) {
    return { ok: false, error: 'UNSUPPORTED_VERSION' };
  }

  if (!isNonEmptyString(obj.url) || !isNonEmptyString(obj.user) || !isNonEmptyString(obj.pwd)) {
    return { ok: false, error: 'MISSING_FIELD' };
  }

  if (!/^https?:\/\//.test(obj.url)) {
    return { ok: false, error: 'INVALID_URL' };
  }

  let label: string | undefined;
  if (obj.o && typeof obj.o === 'object') {
    const oLabel = (obj.o as Record<string, unknown>).label;
    if (isNonEmptyString(oLabel)) label = oLabel;
  }

  return {
    ok: true,
    value: {
      url: obj.url,
      user: obj.user,
      pwd: obj.pwd,
      ...(label !== undefined ? { label } : {}),
    },
  };
}
