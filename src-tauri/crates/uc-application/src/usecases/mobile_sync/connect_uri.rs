//! `mobile_sync::connect_uri` —— `uniclipboard://connect` 深链协议 v1 的
//! 编解码纯函数。
//!
//! ## 为什么需要这个模块
//!
//! 桌面端注册移动设备时, 需要把 `base_url / username / password` + 扩展
//! 元数据塞进一个二维码, 让 iOS Shortcut / Android 兼容客户端 / 未来原生
//! App 用同一套规则解析, 实现"扫码即接入"。规范单一真相是
//! `docs/architecture/mobile-sync-connect-uri.md` —— 任何修改本模块前必须
//! 先同步更新规范文档与 §7 的 golden vector, 再回到这里改实现, 保证 Rust
//! 与 TS (`src/lib/mobileSyncConnectUri.ts`, 阶段 3) 跨语言字节级一致。
//!
//! 模块只做纯编解码:
//! - 不读 settings / 不发起 IO / 不用随机数
//! - 不做 url 可达性探测 / 不做密码强度校验 (`register_device.rs` 负责)
//! - 不持久化任何字段
//!
//! 一切语义错误翻译为 [`ConnectUriError`], 与规范 §4.2 错误码表一一对应。

use std::collections::BTreeMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

// ─── public types ───────────────────────────────────────────────────────

/// v1 payload 的结构化形态。
///
/// 字段定义顺序 (`v / url / user / pwd / o`) 必须与规范 §3.1 一致 —— serde
/// 默认按字段定义顺序序列化, 加上 `o` 用 [`BTreeMap`] 保证字典序, 才能让
/// build 出的字符串在 Rust 与 TS 之间字节相等(规范 §7 golden vector 比对)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConnectPayload {
    /// payload schema 版本; v1 = 1。与 URI envelope `v` 区分(规范 §3.4)。
    pub(crate) v: u32,
    /// 服务端 base URL, 形如 `http://192.168.1.5:42720`, 不带尾斜杠。
    ///
    /// `serde(default)`: 字段缺失时回填空字符串, 让后置 `MissingField` 检查
    /// 统一处理"缺失"和"空字符串"两种语义(规范 §4.2 错误码归并)。
    #[serde(default)]
    pub(crate) url: String,
    /// HTTP Basic Auth 用户名。`serde(default)` 同上。
    #[serde(default)]
    pub(crate) user: String,
    /// HTTP Basic Auth 明文密码 —— 一次性显示语义见规范 §5.1。`serde(default)` 同上。
    #[serde(default)]
    pub(crate) pwd: String,
    /// 扩展元数据 KV。
    /// - 生成侧由 [`ConnectUriOther`] 类型约束写入白名单字段(规范 §3.2)
    /// - 解析侧宽松接受任意字符串 KV, 调用方应忽略未识别的键
    /// - 序列化时 `BTreeMap` 天然字典序输出, 保证跨语言字节一致
    /// - 空 map 时不序列化, 避免 `"o":{}` 让 base64 字节漂移
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) o: BTreeMap<String, String>,
}

/// 生成侧 `o` 字段白名单 —— 类型层面强约束, 避免误把 daemon bearer / 加密
/// passphrase 等敏感字段塞进 QR(规范 §5.2)。
///
/// 新增字段必须先更新规范文档 §3.2 表格, 再在这里添加, 不允许 ad-hoc 扩展。
#[derive(Debug, Default, Clone)]
pub(crate) struct ConnectUriOther {
    /// 设备显示标签, 用于客户端 UI(规范 §3.2)。
    pub(crate) label: Option<String>,
    /// 服务端 device_id, 用于日志关联(规范 §3.2)。
    pub(crate) did: Option<String>,
    /// 协议族提示, v1 仅 `"syncclipboard"`(规范 §3.2)。
    pub(crate) proto: Option<String>,
    /// iOS Shortcut 模板提示(规范 §3.2)。
    pub(crate) install: Option<String>,
}

impl ConnectUriOther {
    /// 转成 BTreeMap, 仅保留 Some 字段; 字典序由 BTreeMap 天然保证。
    fn into_map(self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        if let Some(v) = self.did {
            m.insert("did".into(), v);
        }
        if let Some(v) = self.install {
            m.insert("install".into(), v);
        }
        if let Some(v) = self.label {
            m.insert("label".into(), v);
        }
        if let Some(v) = self.proto {
            m.insert("proto".into(), v);
        }
        m
    }
}

/// build / parse 公共失败语义 —— 错误码与规范 §4.2 表一一对应。
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum ConnectUriError {
    /// scheme ≠ `uniclipboard` 或 host ≠ `connect`(规范 §4.2 `INVALID_SCHEME`)。
    /// 仅 [`parse_mobile_sync_connect_uri`] 构造; build 路径不可能产生。
    #[error("invalid scheme or host (must be uniclipboard://connect)")]
    #[allow(dead_code)]
    // build 路径不构造; parse 路径单测 + 跨语言契约 + 未来 v2 daemon 接收侧使用。
    InvalidScheme,

    /// URI `v` ≠ 1 或 payload `v` ≠ 1(规范 §4.2 `UNSUPPORTED_VERSION`)。
    /// 仅 [`parse_mobile_sync_connect_uri`] 构造。
    #[error("unsupported version (only v=1 is supported)")]
    #[allow(dead_code)] // 同 InvalidScheme: parse-only variant, 保留供前向兼容路径。
    UnsupportedVersion,

    /// URI `svc` ≠ `mobile-sync`(规范 §4.2 `UNSUPPORTED_SERVICE`)。
    /// 仅 [`parse_mobile_sync_connect_uri`] 构造。
    #[error("unsupported service (only svc=mobile-sync is supported)")]
    #[allow(dead_code)] // 同 InvalidScheme: parse-only variant, 保留供前向兼容路径。
    UnsupportedService,

    /// `p` 缺失 / base64url 损坏 / JSON 解析失败(规范 §4.2 `PAYLOAD_DECODE_FAILED`)。
    #[error("payload decode failed: {0}")]
    PayloadDecodeFailed(String),

    /// `url`/`user`/`pwd` 缺失或为空字符串(规范 §4.2 `MISSING_FIELD`)。
    #[error("required field missing or empty: {0}")]
    MissingField(&'static str),

    /// `url` 不以 `http://` 或 `https://` 开头(规范 §4.2 `INVALID_URL`)。
    #[error("invalid url: must start with http:// or https://")]
    InvalidUrl,

    /// 生成侧自检: URI 超过 [`URI_MAX_LEN`] 字符上限(规范 §2)。仅 build 路径出现。
    #[error("uri too long ({len} chars, max {max})")]
    UriTooLong { len: usize, max: usize },
}

// ─── constants ──────────────────────────────────────────────────────────

/// 规范 §2 单一 scheme。
const SCHEME: &str = "uniclipboard";
/// 规范 §2 host。
const HOST: &str = "connect";
/// 当前 URI envelope 版本(规范 §2)。
const ENVELOPE_VERSION: u32 = 1;
/// 当前服务标识(规范 §2)。
const SERVICE: &str = "mobile-sync";
/// payload schema 版本(规范 §3.4)。
const PAYLOAD_VERSION: u32 = 1;
/// 规范 §2 URI 长度上限(易扫描 + 防 `o` 滥用)。
pub(crate) const URI_MAX_LEN: usize = 800;

// ─── build ──────────────────────────────────────────────────────────────

/// 把凭据 + 元数据编码成 `uniclipboard://connect?v=1&svc=mobile-sync&p=<…>`。
///
/// 失败语义:
/// - [`ConnectUriError::MissingField`] 当 url/user/pwd 为空字符串
/// - [`ConnectUriError::InvalidUrl`] 当 url 不以 `http://` 或 `https://` 开头
/// - [`ConnectUriError::UriTooLong`] 当结果超过 [`URI_MAX_LEN`] 字符
///
/// 不负责的事(它们属于 use case 层):
/// - 不做 url 可达性探测
/// - 不做密码强度校验(`register_device.rs` 已做)
/// - 不做 device_id 唯一性检查
pub(crate) fn build_mobile_sync_connect_uri(
    base_url: &str,
    username: &str,
    password: &str,
    other: ConnectUriOther,
) -> Result<String, ConnectUriError> {
    if base_url.is_empty() {
        return Err(ConnectUriError::MissingField("url"));
    }
    if username.is_empty() {
        return Err(ConnectUriError::MissingField("user"));
    }
    if password.is_empty() {
        return Err(ConnectUriError::MissingField("pwd"));
    }
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(ConnectUriError::InvalidUrl);
    }

    let payload = ConnectPayload {
        v: PAYLOAD_VERSION,
        url: base_url.to_string(),
        user: username.to_string(),
        pwd: password.to_string(),
        o: other.into_map(),
    };

    // serde_json::to_string 默认 minify(无 indent 即无空白); 字段顺序按 struct
    // 定义; BTreeMap 序列化为字典序。三者合起来保证跨语言字节稳定。
    let json = serde_json::to_string(&payload)
        .map_err(|e| ConnectUriError::PayloadDecodeFailed(format!("serialize: {e}")))?;
    let p = URL_SAFE_NO_PAD.encode(json.as_bytes());

    let uri = format!("{SCHEME}://{HOST}?v={ENVELOPE_VERSION}&svc={SERVICE}&p={p}");

    if uri.len() > URI_MAX_LEN {
        return Err(ConnectUriError::UriTooLong {
            len: uri.len(),
            max: URI_MAX_LEN,
        });
    }

    Ok(uri)
}

// ─── parse ──────────────────────────────────────────────────────────────

/// 把 QR 文本反向解码出 payload。错误码与规范 §4.2 一一对应。
///
/// 当前 use case 路径只走 [`build_mobile_sync_connect_uri`](self::build_mobile_sync_connect_uri),
/// 不调 parse —— iOS Shortcut / Android 客户端在自己端各自实现解码。
/// 本函数保留用于:
/// 1. 本模块单测的 round-trip 断言;
/// 2. `register_device.rs` 跨模块测试中验证 happy-path 输出语义;
/// 3. 未来 v2 daemon "扫码回执"路径(规范 §10) 的接收侧解析复用;
/// 4. 跨语言契约 (`src/lib/mobileSyncConnectUri.ts` 阶段 3) 的字节级对照。
///
/// 不负责:
/// - 不发起 HTTP 探活(可选, 由调用方决定)
/// - 不持久化任何字段
/// - 不修剪 pwd 前后空白(规范 §3.1: pwd 任何字节都合法)
#[allow(dead_code)] // 仅测试 / 未来 v2 路径使用; 阶段 2 不接入生产消费侧。
pub(crate) fn parse_mobile_sync_connect_uri(
    qr_text: &str,
) -> Result<ConnectPayload, ConnectUriError> {
    let raw = qr_text.trim();

    let uri = Url::parse(raw).map_err(|_| ConnectUriError::InvalidScheme)?;
    if uri.scheme() != SCHEME {
        return Err(ConnectUriError::InvalidScheme);
    }
    if uri.host_str() != Some(HOST) {
        return Err(ConnectUriError::InvalidScheme);
    }

    let mut q_v: Option<String> = None;
    let mut q_svc: Option<String> = None;
    let mut q_p: Option<String> = None;
    for (k, v) in uri.query_pairs() {
        match k.as_ref() {
            "v" => q_v = Some(v.into_owned()),
            "svc" => q_svc = Some(v.into_owned()),
            "p" => q_p = Some(v.into_owned()),
            // 前向兼容: 忽略未识别的 query 键(规范 §3.2 同款 ignore-unknown 思路)。
            _ => {}
        }
    }

    let envelope_v: u32 = q_v
        .ok_or(ConnectUriError::UnsupportedVersion)?
        .parse()
        .map_err(|_| ConnectUriError::UnsupportedVersion)?;
    if envelope_v != ENVELOPE_VERSION {
        return Err(ConnectUriError::UnsupportedVersion);
    }
    if q_svc.as_deref() != Some(SERVICE) {
        return Err(ConnectUriError::UnsupportedService);
    }
    let p = q_p
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ConnectUriError::PayloadDecodeFailed("p missing or empty".into()))?;

    let json_bytes = URL_SAFE_NO_PAD
        .decode(p.as_bytes())
        .map_err(|e| ConnectUriError::PayloadDecodeFailed(format!("base64url: {e}")))?;
    let payload: ConnectPayload = serde_json::from_slice(&json_bytes)
        .map_err(|e| ConnectUriError::PayloadDecodeFailed(format!("json: {e}")))?;

    if payload.v != PAYLOAD_VERSION {
        return Err(ConnectUriError::UnsupportedVersion);
    }
    if payload.url.is_empty() {
        return Err(ConnectUriError::MissingField("url"));
    }
    if payload.user.is_empty() {
        return Err(ConnectUriError::MissingField("user"));
    }
    if payload.pwd.is_empty() {
        return Err(ConnectUriError::MissingField("pwd"));
    }
    if !(payload.url.starts_with("http://") || payload.url.starts_with("https://")) {
        return Err(ConnectUriError::InvalidUrl);
    }

    Ok(payload)
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// happy-path golden vector 与规范 §7.1 完全一致 —— 任一处变动须同步两侧
    /// (与 `src/lib/__tests__/mobileSyncConnectUri.test.ts` 阶段 3 测试对齐)。
    const GOLDEN_URI: &str = "uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ";

    fn golden_other() -> ConnectUriOther {
        ConnectUriOther {
            label: Some("Test".into()),
            did: Some("did_0123abcd".into()),
            proto: Some("syncclipboard".into()),
            install: None,
        }
    }

    // ── build: happy + 字节稳定 ────────────────────────────────────────

    #[test]
    fn build_emits_golden_uri() {
        let uri = build_mobile_sync_connect_uri(
            "http://192.168.1.5:42720",
            "mobile_aabbccdd",
            "AbCdEfGhIjKlMnOpQrSt",
            golden_other(),
        )
        .expect("build must succeed for golden inputs");
        assert_eq!(uri, GOLDEN_URI);
    }

    #[test]
    fn build_drops_empty_other_map() {
        // 没有 o 字段时, JSON 不应出现 "o":{}, 否则 base64 字节会漂移。
        let uri =
            build_mobile_sync_connect_uri("http://a.b", "user", "pass", ConnectUriOther::default())
                .expect("build must succeed");
        let p = uri.split("p=").nth(1).expect("p param present");
        let bytes = URL_SAFE_NO_PAD.decode(p).expect("base64 ok");
        let json = std::str::from_utf8(&bytes).expect("utf8");
        assert!(
            !json.contains("\"o\""),
            "json should not contain 'o': {json}"
        );
    }

    #[test]
    fn build_orders_other_keys_lexicographically() {
        // 即便 ConnectUriOther 字段填入顺序与字典序不同, BTreeMap 也会强制
        // 输出 did → install → label → proto。
        let other = ConnectUriOther {
            proto: Some("syncclipboard".into()),
            label: Some("L".into()),
            did: Some("D".into()),
            install: Some("I".into()),
        };
        let uri = build_mobile_sync_connect_uri("http://a.b", "user", "pwd", other).unwrap();
        let p = uri.split("p=").nth(1).unwrap();
        let json = String::from_utf8(URL_SAFE_NO_PAD.decode(p).unwrap()).unwrap();
        let did_pos = json.find("\"did\"").expect("did present");
        let install_pos = json.find("\"install\"").expect("install present");
        let label_pos = json.find("\"label\"").expect("label present");
        let proto_pos = json.find("\"proto\"").expect("proto present");
        assert!(did_pos < install_pos);
        assert!(install_pos < label_pos);
        assert!(label_pos < proto_pos);
    }

    // ── build: 负例 ────────────────────────────────────────────────────

    #[test]
    fn build_rejects_empty_url() {
        let err = build_mobile_sync_connect_uri("", "user", "pwd", ConnectUriOther::default())
            .unwrap_err();
        assert_eq!(err, ConnectUriError::MissingField("url"));
    }

    #[test]
    fn build_rejects_empty_user() {
        let err =
            build_mobile_sync_connect_uri("http://a.b", "", "pwd", ConnectUriOther::default())
                .unwrap_err();
        assert_eq!(err, ConnectUriError::MissingField("user"));
    }

    #[test]
    fn build_rejects_empty_pwd() {
        let err =
            build_mobile_sync_connect_uri("http://a.b", "user", "", ConnectUriOther::default())
                .unwrap_err();
        assert_eq!(err, ConnectUriError::MissingField("pwd"));
    }

    #[test]
    fn build_rejects_non_http_url() {
        let err =
            build_mobile_sync_connect_uri("ftp://a.b", "user", "pwd", ConnectUriOther::default())
                .unwrap_err();
        assert_eq!(err, ConnectUriError::InvalidUrl);
    }

    #[test]
    fn build_rejects_uri_too_long() {
        let other = ConnectUriOther {
            label: Some("L".repeat(1000)),
            ..Default::default()
        };
        let err = build_mobile_sync_connect_uri("http://a.b", "user", "pwd", other).unwrap_err();
        match err {
            ConnectUriError::UriTooLong { max, .. } => assert_eq!(max, URI_MAX_LEN),
            other => panic!("expected UriTooLong, got {other:?}"),
        }
    }

    // ── parse: happy + 修剪 ────────────────────────────────────────────

    #[test]
    fn parse_golden_round_trips() {
        let p = parse_mobile_sync_connect_uri(GOLDEN_URI).expect("parse ok");
        assert_eq!(p.v, 1);
        assert_eq!(p.url, "http://192.168.1.5:42720");
        assert_eq!(p.user, "mobile_aabbccdd");
        assert_eq!(p.pwd, "AbCdEfGhIjKlMnOpQrSt");
        assert_eq!(p.o.get("did").map(String::as_str), Some("did_0123abcd"));
        assert_eq!(p.o.get("label").map(String::as_str), Some("Test"));
        assert_eq!(p.o.get("proto").map(String::as_str), Some("syncclipboard"));
        assert_eq!(p.o.len(), 3);
    }

    #[test]
    fn parse_trims_whitespace() {
        let with_ws = format!("  \n{GOLDEN_URI}\t  ");
        parse_mobile_sync_connect_uri(&with_ws).expect("trim ok");
    }

    // ── parse: 负例 (一一对应规范 §7.2) ────────────────────────────────

    #[test]
    fn parse_rejects_wrong_scheme_https() {
        // §7.2 #1: https URL 不是本协议。
        let err = parse_mobile_sync_connect_uri(
            "https://example.com/connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ",
        )
        .unwrap_err();
        assert_eq!(err, ConnectUriError::InvalidScheme);
    }

    #[test]
    fn parse_rejects_uniclip_alias() {
        // v1 决定: 单一 scheme, `uniclip://` alias 必须被拒绝。
        let err =
            parse_mobile_sync_connect_uri("uniclip://connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ")
                .unwrap_err();
        assert_eq!(err, ConnectUriError::InvalidScheme);
    }

    #[test]
    fn parse_rejects_wrong_host() {
        let err =
            parse_mobile_sync_connect_uri("uniclipboard://other?v=1&svc=mobile-sync&p=eyJ2IjoxfQ")
                .unwrap_err();
        assert_eq!(err, ConnectUriError::InvalidScheme);
    }

    #[test]
    fn parse_rejects_unsupported_envelope_v() {
        // §7.2 #2
        let err = parse_mobile_sync_connect_uri(
            "uniclipboard://connect?v=2&svc=mobile-sync&p=eyJ2IjoxfQ",
        )
        .unwrap_err();
        assert_eq!(err, ConnectUriError::UnsupportedVersion);
    }

    #[test]
    fn parse_rejects_unsupported_service() {
        // §7.2 #3
        let err =
            parse_mobile_sync_connect_uri("uniclipboard://connect?v=1&svc=other&p=eyJ2IjoxfQ")
                .unwrap_err();
        assert_eq!(err, ConnectUriError::UnsupportedService);
    }

    #[test]
    fn parse_rejects_malformed_base64() {
        // §7.2 #4
        let err = parse_mobile_sync_connect_uri(
            "uniclipboard://connect?v=1&svc=mobile-sync&p=not-valid-base64!@#",
        )
        .unwrap_err();
        match err {
            ConnectUriError::PayloadDecodeFailed(_) => {}
            other => panic!("expected PayloadDecodeFailed, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_missing_pwd() {
        // §7.2 #5: base64 of {"v":1,"url":"http://a.b","user":"u"}
        let err = parse_mobile_sync_connect_uri(
            "uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vYS5iIiwidXNlciI6InUifQ",
        )
        .unwrap_err();
        assert_eq!(err, ConnectUriError::MissingField("pwd"));
    }

    #[test]
    fn parse_rejects_non_http_url_in_payload() {
        // §7.2 #6: ftp scheme. base64 of {"v":1,"url":"ftp://a.b","user":"u","pwd":"p"}
        let err = parse_mobile_sync_connect_uri(
            "uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJmdHA6Ly9hLmIiLCJ1c2VyIjoidSIsInB3ZCI6InAifQ",
        )
        .unwrap_err();
        assert_eq!(err, ConnectUriError::InvalidUrl);
    }

    // ── parse: 其它边界 ────────────────────────────────────────────────

    #[test]
    fn parse_rejects_missing_p_param() {
        let err = parse_mobile_sync_connect_uri("uniclipboard://connect?v=1&svc=mobile-sync")
            .unwrap_err();
        match err {
            ConnectUriError::PayloadDecodeFailed(_) => {}
            other => panic!("expected PayloadDecodeFailed, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_payload_v_mismatch() {
        // payload.v=2 但 envelope v=1 —— 后置 payload 检查必须报 UnsupportedVersion。
        let payload = r#"{"v":2,"url":"http://a.b","user":"u","pwd":"p"}"#;
        let p = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let uri = format!("uniclipboard://connect?v=1&svc=mobile-sync&p={p}");
        let err = parse_mobile_sync_connect_uri(&uri).unwrap_err();
        assert_eq!(err, ConnectUriError::UnsupportedVersion);
    }

    #[test]
    fn parse_ignores_unknown_o_keys() {
        // 前向兼容: 未来加入新 o.* 键时, 老解析器不报错, 直接保留在 BTreeMap
        // 中供调用方按需消费(规范 §3.2 ignore-unknown 思路)。
        let payload = r#"{"v":1,"url":"http://a.b","user":"u","pwd":"p","o":{"future_key":"future_val","label":"L"}}"#;
        let p = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let uri = format!("uniclipboard://connect?v=1&svc=mobile-sync&p={p}");
        let parsed = parse_mobile_sync_connect_uri(&uri).expect("forward-compat ok");
        assert_eq!(
            parsed.o.get("future_key").map(String::as_str),
            Some("future_val")
        );
        assert_eq!(parsed.o.get("label").map(String::as_str), Some("L"));
    }

    // ── round-trip: build → parse → 字段一致 ───────────────────────────

    #[test]
    fn build_parse_round_trip_preserves_fields() {
        let other = ConnectUriOther {
            label: Some("我的 iPhone".into()),
            did: Some("did_xyz".into()),
            proto: Some("syncclipboard".into()),
            install: Some("shortcut-ex".into()),
        };
        let uri = build_mobile_sync_connect_uri(
            "http://10.0.0.5:42720",
            "alice_001",
            "p@ssw0rd-with-symbols",
            other,
        )
        .expect("build ok");
        let parsed = parse_mobile_sync_connect_uri(&uri).expect("parse ok");
        assert_eq!(parsed.url, "http://10.0.0.5:42720");
        assert_eq!(parsed.user, "alice_001");
        assert_eq!(parsed.pwd, "p@ssw0rd-with-symbols");
        assert_eq!(
            parsed.o.get("label").map(String::as_str),
            Some("我的 iPhone")
        );
        assert_eq!(parsed.o.get("did").map(String::as_str), Some("did_xyz"));
        assert_eq!(
            parsed.o.get("install").map(String::as_str),
            Some("shortcut-ex")
        );
    }
}
