import {
  parseConnectUri,
  CONNECT_URI_ERROR_MESSAGES,
  type ConnectUriError,
} from '@/utils/connectUri';

// spec 正向 golden vector
const VALID_URI =
  'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ';

describe('parseConnectUri — positive', () => {
  it('解码 spec 正向向量并还原字段', () => {
    const r = parseConnectUri(VALID_URI);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.value).toEqual({
        url: 'http://192.168.1.5:42720',
        user: 'mobile_aabbccdd',
        pwd: 'AbCdEfGhIjKlMnOpQrSt',
        label: 'Test',
      });
    }
  });

  it('容忍输入前后空白', () => {
    const r = parseConnectUri(`  \n${VALID_URI}\t `);
    expect(r.ok).toBe(true);
  });

  it('payload 无 o 时不抛错，label 缺失', () => {
    // {"v":1,"url":"http://a.b","user":"abcdef","pwd":"p"} → base64url
    const json = '{"v":1,"url":"http://a.b","user":"abcdef","pwd":"p"}';
    const p = Buffer.from(json, 'utf-8')
      .toString('base64')
      .replace(/=+$/, '')
      .replace(/\+/g, '-')
      .replace(/\//g, '_');
    const uri = `uniclipboard://connect?v=1&svc=mobile-sync&p=${p}`;
    const r = parseConnectUri(uri);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.value.label).toBeUndefined();
      expect(r.value.url).toBe('http://a.b');
    }
  });

  it('o.label 非字符串时被忽略', () => {
    const json = '{"v":1,"url":"http://a.b","user":"abcdef","pwd":"p","o":{"label":42}}';
    const p = Buffer.from(json, 'utf-8')
      .toString('base64')
      .replace(/=+$/, '')
      .replace(/\+/g, '-')
      .replace(/\//g, '_');
    const r = parseConnectUri(`uniclipboard://connect?v=1&svc=mobile-sync&p=${p}`);
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.value.label).toBeUndefined();
  });
});

describe('parseConnectUri — negative (spec 反向向量)', () => {
  const cases: Array<{ name: string; input: string; expected: ConnectUriError }> = [
    {
      name: '#1 错误 scheme (https)',
      input: 'https://example.com/connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ',
      expected: 'INVALID_SCHEME',
    },
    {
      name: '#2 URI v=2',
      input: 'uniclipboard://connect?v=2&svc=mobile-sync&p=eyJ2IjoxfQ',
      expected: 'UNSUPPORTED_VERSION',
    },
    {
      name: '#3 svc=other',
      input: 'uniclipboard://connect?v=1&svc=other&p=eyJ2IjoxfQ',
      expected: 'UNSUPPORTED_SERVICE',
    },
    {
      name: '#4 base64url 坏',
      input: 'uniclipboard://connect?v=1&svc=mobile-sync&p=not-valid-base64!@#',
      expected: 'PAYLOAD_DECODE_FAILED',
    },
    {
      name: '#5 缺 pwd',
      input:
        'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vYS5iIiwidXNlciI6InUifQ',
      expected: 'MISSING_FIELD',
    },
    {
      name: '#6 ftp scheme',
      input:
        'uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJmdHA6Ly9hLmIiLCJ1c2VyIjoidSIsInB3ZCI6InAifQ',
      expected: 'INVALID_URL',
    },
  ];

  test.each(cases)('$name → $expected', ({ input, expected }) => {
    const r = parseConnectUri(input);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toBe(expected);
  });
});

describe('parseConnectUri — 边界', () => {
  it('空串 → INVALID_SCHEME', () => {
    const r = parseConnectUri('');
    expect(r).toEqual({ ok: false, error: 'INVALID_SCHEME' });
  });

  it('uniclipboard scheme 但 host 不是 connect → INVALID_SCHEME', () => {
    const r = parseConnectUri('uniclipboard://other?v=1&svc=mobile-sync&p=eyJ2IjoxfQ');
    expect(r).toEqual({ ok: false, error: 'INVALID_SCHEME' });
  });

  it('v 非数字 → UNSUPPORTED_VERSION', () => {
    const r = parseConnectUri('uniclipboard://connect?v=abc&svc=mobile-sync&p=eyJ2IjoxfQ');
    expect(r).toEqual({ ok: false, error: 'UNSUPPORTED_VERSION' });
  });

  it('缺失 p → PAYLOAD_DECODE_FAILED', () => {
    const r = parseConnectUri('uniclipboard://connect?v=1&svc=mobile-sync');
    expect(r).toEqual({ ok: false, error: 'PAYLOAD_DECODE_FAILED' });
  });

  it('payload v=2 → UNSUPPORTED_VERSION', () => {
    // {"v":2,"url":"http://a.b","user":"abcdef","pwd":"p"}
    const json = '{"v":2,"url":"http://a.b","user":"abcdef","pwd":"p"}';
    const p = Buffer.from(json, 'utf-8')
      .toString('base64')
      .replace(/=+$/, '')
      .replace(/\+/g, '-')
      .replace(/\//g, '_');
    const r = parseConnectUri(`uniclipboard://connect?v=1&svc=mobile-sync&p=${p}`);
    expect(r).toEqual({ ok: false, error: 'UNSUPPORTED_VERSION' });
  });

  it('payload 是 JSON 但不是对象 → PAYLOAD_DECODE_FAILED', () => {
    const json = '[1,2,3]';
    const p = Buffer.from(json, 'utf-8')
      .toString('base64')
      .replace(/=+$/, '')
      .replace(/\+/g, '-')
      .replace(/\//g, '_');
    const r = parseConnectUri(`uniclipboard://connect?v=1&svc=mobile-sync&p=${p}`);
    expect(r).toEqual({ ok: false, error: 'PAYLOAD_DECODE_FAILED' });
  });
});

describe('CONNECT_URI_ERROR_MESSAGES', () => {
  it('每个错误码都有非空中文文案', () => {
    const codes: ConnectUriError[] = [
      'INVALID_SCHEME',
      'UNSUPPORTED_VERSION',
      'UNSUPPORTED_SERVICE',
      'PAYLOAD_DECODE_FAILED',
      'MISSING_FIELD',
      'INVALID_URL',
    ];
    for (const c of codes) {
      expect(typeof CONNECT_URI_ERROR_MESSAGES[c]).toBe('string');
      expect(CONNECT_URI_ERROR_MESSAGES[c].length).toBeGreaterThan(0);
    }
  });
});
