//! `AuthenticateMobileRequestUseCase` —— 校验一条 LAN HTTP 业务请求的 4 个
//! 鉴权 header（Bearer token + timestamp + nonce + signature），返回对应已
//! 登记的 `MobileDevice`。
//!
//! 这是 SPEC §4.3 五步校验在应用层的"权威实现"——uc-webserver 的 axum
//! middleware 不重复定义校验流程，只解析 header 后调用本 use case。**所有
//! 协议层规则（错误码语义 / 错误顺序 / 签名输入串构成）都收口在这里**，
//! webserver 只负责"传字节进来 + 把错误翻成 HTTP status"。
//!
//! ## 校验顺序（短路返回首个失败）
//!
//! 1. **格式校验**：token_hex 必须 64 字符 hex，body_hash_hex 必须 64 字符
//!    hex，nonce 非空，否则 `InvalidTokenFormat` / `InvalidBodyHashFormat`
//!    / `InvalidNonceFormat`。
//! 2. **token 查表**：按 SHA-256(token_bytes) 找 device；找不到 →
//!    `InvalidToken`（设备已撤销或 token 错）。
//! 3. **timestamp 漂移**：|now - ts| > drift_tolerance_ms → `TimestampDrift`。
//! 4. **nonce 防重放**：写入滑动窗口；命中 → `NonceReplay`；窗口满 →
//!    `NonceCacheFull`。注意 nonce 校验放在签名校验之前是有意为之 ——
//!    没必要花算力算签名给重放/超时请求。
//! 5. **签名校验**：按 SPEC §4.3 拼 `${token}\n${ts}\n${nonce}\n${METHOD}
//!    \n${path}\n${body_hash}`，SHA-256 后 hex；与 header 中签名常时间比较
//!    （`subtle::ConstantTimeEq`）。不一致 → `InvalidSignature`。
//!
//! ## 不在这里做的事
//!
//! - **不**写 `record_activity`：属于"鉴权成功后的副作用"，由 middleware /
//!   handler 决定何时登记（避免一次请求多次写盘）。
//! - **不**关心 body 大小限制 / mime 白名单：SPEC §5 的协议规则归 handler
//!   分支。本 use case 只回答"这台设备能不能进门"。

use std::sync::Arc;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tracing::{instrument, warn};

use uc_core::mobile_sync::{MobileDevice, MobileDeviceError, TokenHash};
use uc_core::ports::{ClockPort, MobileDeviceRepositoryPort, NonceError, NoncePort};

/// 默认时间戳漂移容忍：60 秒。与 `InMemoryNonceCache` 默认 TTL 同值，
/// 形成"过期窗口 = 防重放窗口"的对称。
pub const DEFAULT_TIMESTAMP_DRIFT_TOLERANCE_MS: i64 = 60_000;

// ─── 输入 / 输出 ────────────────────────────────────────────────────────

/// LAN HTTP 鉴权 4 个 header 的解析后结构。
///
/// header 名 / 大小写规则在 webserver middleware 里处理；本结构只接收已经
/// "解码到字符串/数字"的字段。
#[derive(Debug, Clone)]
pub struct MobileAuthHeaders {
    /// `Authorization: Bearer <hex>` 中 hex 部分（64 字符）。
    pub token_hex: String,
    /// `X-UC-Timestamp` 头解析的毫秒数（i64，允许负值由漂移检查兜住）。
    pub timestamp_ms: i64,
    /// `X-UC-Nonce` 头原文。
    pub nonce: String,
    /// `X-UC-Signature` 头原文（64 字符 hex）。
    pub signature_hex: String,
}

#[derive(Debug, Clone)]
pub struct AuthenticateMobileRequestInput {
    pub headers: MobileAuthHeaders,
    /// HTTP method —— 大写（`GET` / `PUT` 等）。middleware 必须传统一大小写。
    pub method: String,
    /// 完整 path（含 query 部分；middleware 应传 `request.uri().path_and_query()`）。
    pub path: String,
    /// body 的 SHA-256 hex（64 字符 lowercase）。空 body 用 SHA-256("").hex。
    pub body_hash_hex: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthenticateMobileRequestError {
    /// token_hex 不是 64 字符 lowercase hex。
    #[error("invalid token format")]
    InvalidTokenFormat,
    /// body_hash_hex 不是 64 字符 hex（middleware 算出来的 hash 应总是 64 字
    /// 符；走到这里基本是 middleware bug）。
    #[error("invalid body hash format")]
    InvalidBodyHashFormat,
    /// nonce 为空 / 含异常字符。SPEC 不强制 nonce 形态，这里只挡空字符串。
    #[error("invalid nonce format")]
    InvalidNonceFormat,
    /// signature_hex 不是 64 字符 hex（middleware 通常已经过滤；这里兜底）。
    #[error("invalid signature format")]
    InvalidSignatureFormat,
    /// token 不存在 / 已撤销。
    #[error("invalid token")]
    InvalidToken,
    /// `|now - ts| > drift_tolerance_ms`。
    #[error("timestamp drift")]
    TimestampDrift,
    /// nonce 在窗口内已被见过，疑似重放。
    #[error("nonce replay")]
    NonceReplay,
    /// nonce 缓存已满，503 nonce_cache_full。
    #[error("nonce cache full")]
    NonceCacheFull,
    /// 签名校验失败。
    #[error("invalid signature")]
    InvalidSignature,
    /// 下层存储异常（device repo 或 nonce store）。adapter 层文案带上来。
    #[error("storage failure: {0}")]
    Storage(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct AuthenticateMobileRequestUseCase {
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    nonces: Arc<dyn NoncePort>,
    clock: Arc<dyn ClockPort>,
    drift_tolerance_ms: i64,
}

impl AuthenticateMobileRequestUseCase {
    pub(crate) fn new(
        device_repo: Arc<dyn MobileDeviceRepositoryPort>,
        nonces: Arc<dyn NoncePort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self::with_drift(
            device_repo,
            nonces,
            clock,
            DEFAULT_TIMESTAMP_DRIFT_TOLERANCE_MS,
        )
    }

    pub(crate) fn with_drift(
        device_repo: Arc<dyn MobileDeviceRepositoryPort>,
        nonces: Arc<dyn NoncePort>,
        clock: Arc<dyn ClockPort>,
        drift_tolerance_ms: i64,
    ) -> Self {
        Self {
            device_repo,
            nonces,
            clock,
            drift_tolerance_ms,
        }
    }

    #[instrument(skip(self, input), fields(method = %input.method, path = %input.path))]
    pub(crate) async fn execute(
        &self,
        input: AuthenticateMobileRequestInput,
    ) -> Result<MobileDevice, AuthenticateMobileRequestError> {
        // ── 1. 格式校验 ─────────────────────────────────────────────────
        if !is_lowercase_hex_64(&input.headers.token_hex) {
            return Err(AuthenticateMobileRequestError::InvalidTokenFormat);
        }
        if !is_hex_64(&input.body_hash_hex) {
            return Err(AuthenticateMobileRequestError::InvalidBodyHashFormat);
        }
        if input.headers.nonce.is_empty() {
            return Err(AuthenticateMobileRequestError::InvalidNonceFormat);
        }
        if !is_hex_64(&input.headers.signature_hex) {
            return Err(AuthenticateMobileRequestError::InvalidSignatureFormat);
        }

        // ── 2. token 查表 ────────────────────────────────────────────────
        let token_bytes =
            decode_hex_32(&input.headers.token_hex).expect("hex_64 already validated");
        let token_hash = TokenHash::new(sha256_32(&token_bytes));
        let device = self
            .device_repo
            .find_by_token_hash(&token_hash)
            .await
            .map_err(translate_device_error)?
            .ok_or(AuthenticateMobileRequestError::InvalidToken)?;

        // ── 3. timestamp 漂移 ─────────────────────────────────────────────
        let now_ms = self.clock.now_ms();
        let drift_abs = now_ms
            .saturating_sub(input.headers.timestamp_ms)
            .unsigned_abs();
        if drift_abs > self.drift_tolerance_ms.unsigned_abs() {
            return Err(AuthenticateMobileRequestError::TimestampDrift);
        }

        // ── 4. nonce 防重放 ───────────────────────────────────────────────
        match self
            .nonces
            .record_if_new(&input.headers.nonce, now_ms)
            .await
        {
            Ok(true) => {}
            Ok(false) => return Err(AuthenticateMobileRequestError::NonceReplay),
            Err(NonceError::CacheFull) => {
                warn!("nonce cache full; rejecting authenticated request");
                return Err(AuthenticateMobileRequestError::NonceCacheFull);
            }
            Err(NonceError::Storage(msg)) => {
                return Err(AuthenticateMobileRequestError::Storage(msg))
            }
        }

        // ── 5. 签名校验（最后一步，避免给重放/超时请求白算签名）───────
        let expected_hex = compute_signature_hex(
            &input.headers.token_hex,
            input.headers.timestamp_ms,
            &input.headers.nonce,
            &input.method,
            &input.path,
            &input.body_hash_hex,
        );
        if !ct_eq_ignore_case_hex(&expected_hex, &input.headers.signature_hex) {
            return Err(AuthenticateMobileRequestError::InvalidSignature);
        }

        Ok(device)
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

fn translate_device_error(err: MobileDeviceError) -> AuthenticateMobileRequestError {
    match err {
        MobileDeviceError::Storage(msg) => AuthenticateMobileRequestError::Storage(msg),
        other => AuthenticateMobileRequestError::Storage(other.to_string()),
    }
}

fn is_hex_64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_lowercase_hex_64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn decode_hex_32(s: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    hex::decode_to_slice(s, &mut out).ok()?;
    Some(out)
}

fn sha256_32(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// 拼"`token\nts\nnonce\nmethod\npath\nbody_hash`"再算 SHA-256 hex。
/// 字段顺序与分隔符严格按 SPEC §4.3。
pub(crate) fn compute_signature_hex(
    token_hex: &str,
    timestamp_ms: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body_hash_hex: &str,
) -> String {
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        token_hex, timestamp_ms, nonce, method, path, body_hash_hex
    );
    hex::encode(sha256_32(canonical.as_bytes()))
}

/// hex 字符串的常时间相等比较；先转 lowercase byte slice 再比，免得遇到
/// 大小写差异的临时字符串泄露 cache timing。两边长度不等直接 false（保持
/// 短路是因为长度不同本身就是非密码学事实）。
fn ct_eq_ignore_case_hex(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let aa: Vec<u8> = a.bytes().map(|b| b.to_ascii_lowercase()).collect();
    let bb: Vec<u8> = b.bytes().map(|b| b.to_ascii_lowercase()).collect();
    aa.ct_eq(&bb).into()
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    use uc_core::mobile_sync::{
        MobileClientType, MobileDevice, MobileDeviceError, MobileDeviceId, TokenHash,
    };

    // ── helpers ─────────────────────────────────────────────────────────

    /// `f` 重复 64 次 —— 64 字符的 lowercase hex 占位 token，方便测试不必
    /// 真的算 hash。
    const FAKE_TOKEN_HEX: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    const FAKE_BODY_HASH_HEX: &str =
        "0000000000000000000000000000000000000000000000000000000000000000";

    fn token_hash_for(token_hex: &str) -> TokenHash {
        let bytes = decode_hex_32(token_hex).expect("hex");
        TokenHash::new(sha256_32(&bytes))
    }

    fn fake_device(token_hash: TokenHash) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new("did_test"),
            label: "iPhone".into(),
            client_type: MobileClientType::IosShortcut,
            reported_name: None,
            reported_os: None,
            token_hash,
            created_at_ms: 0,
            last_seen_at_ms: None,
            last_seen_ip: None,
        }
    }

    /// 内存 device repo，按 token_hash 命中预置数据。
    struct StubDeviceRepo {
        device: Option<MobileDevice>,
        force_storage_err: bool,
    }
    #[async_trait]
    impl MobileDeviceRepositoryPort for StubDeviceRepo {
        async fn save(&self, _: &MobileDevice) -> Result<(), MobileDeviceError> {
            unreachable!("auth not exercising save")
        }
        async fn find_by_token_hash(
            &self,
            hash: &TokenHash,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            if self.force_storage_err {
                return Err(MobileDeviceError::Storage("disk gone".into()));
            }
            Ok(self
                .device
                .as_ref()
                .filter(|d| d.token_hash == *hash)
                .cloned())
        }
        async fn find_by_device_id(
            &self,
            _: &MobileDeviceId,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            unreachable!()
        }
        async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
            unreachable!()
        }
        async fn delete(&self, _: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
            unreachable!()
        }
        async fn record_activity(
            &self,
            _: &MobileDeviceId,
            _: i64,
            _: Option<String>,
            _: Option<String>,
            _: Option<String>,
        ) -> Result<(), MobileDeviceError> {
            unreachable!()
        }
    }

    /// 简化版 nonce port：所有未见过的 nonce 都接受，可强制变成"已见"或
    /// "缓存满"。
    #[derive(Default)]
    struct StubNonces {
        seen: Mutex<Vec<String>>,
        force_full: bool,
    }
    #[async_trait]
    impl NoncePort for StubNonces {
        async fn record_if_new(
            &self,
            nonce: &str,
            _observed_at_ms: i64,
        ) -> Result<bool, NonceError> {
            if self.force_full {
                return Err(NonceError::CacheFull);
            }
            let mut g = self.seen.lock().unwrap();
            if g.iter().any(|n| n == nonce) {
                return Ok(false);
            }
            g.push(nonce.to_string());
            Ok(true)
        }
    }

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    fn make_uc(
        device: Option<MobileDevice>,
        nonces: Arc<StubNonces>,
        now_ms: i64,
    ) -> AuthenticateMobileRequestUseCase {
        AuthenticateMobileRequestUseCase::new(
            Arc::new(StubDeviceRepo {
                device,
                force_storage_err: false,
            }),
            nonces,
            Arc::new(FixedClock(now_ms)),
        )
    }

    fn input_with_signature(
        token_hex: &str,
        ts: i64,
        nonce: &str,
        method: &str,
        path: &str,
        body_hash_hex: &str,
    ) -> AuthenticateMobileRequestInput {
        let sig = compute_signature_hex(token_hex, ts, nonce, method, path, body_hash_hex);
        AuthenticateMobileRequestInput {
            headers: MobileAuthHeaders {
                token_hex: token_hex.into(),
                timestamp_ms: ts,
                nonce: nonce.into(),
                signature_hex: sig,
            },
            method: method.into(),
            path: path.into(),
            body_hash_hex: body_hash_hex.into(),
        }
    }

    // ── tests ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn happy_path_returns_device() {
        let hash = token_hash_for(FAKE_TOKEN_HEX);
        let device = fake_device(hash);
        let nonces = Arc::new(StubNonces::default());
        let uc = make_uc(Some(device.clone()), nonces, 1_000);
        let input = input_with_signature(
            FAKE_TOKEN_HEX,
            1_000,
            "n1",
            "GET",
            "/mobile/v1/clipboard/latest",
            FAKE_BODY_HASH_HEX,
        );
        let out = uc.execute(input).await.expect("auth ok");
        assert_eq!(out.device_id, device.device_id);
    }

    #[tokio::test]
    async fn unknown_token_returns_invalid_token() {
        let nonces = Arc::new(StubNonces::default());
        let uc = make_uc(None, nonces, 1_000);
        let input =
            input_with_signature(FAKE_TOKEN_HEX, 1_000, "n1", "GET", "/x", FAKE_BODY_HASH_HEX);
        let err = uc.execute(input).await.unwrap_err();
        assert!(matches!(err, AuthenticateMobileRequestError::InvalidToken));
    }

    #[tokio::test]
    async fn timestamp_drift_outside_window_rejected() {
        let device = fake_device(token_hash_for(FAKE_TOKEN_HEX));
        let nonces = Arc::new(StubNonces::default());
        // now = 1_000_000；header ts = 1_000_000 - 60_001（超出 60 秒）。
        let uc = make_uc(Some(device), nonces, 1_000_000);
        let input = input_with_signature(
            FAKE_TOKEN_HEX,
            1_000_000 - 60_001,
            "n1",
            "GET",
            "/x",
            FAKE_BODY_HASH_HEX,
        );
        let err = uc.execute(input).await.unwrap_err();
        assert!(matches!(
            err,
            AuthenticateMobileRequestError::TimestampDrift
        ));
    }

    #[tokio::test]
    async fn nonce_replay_rejected_after_first_record() {
        let device = fake_device(token_hash_for(FAKE_TOKEN_HEX));
        let nonces = Arc::new(StubNonces::default());
        let uc = make_uc(Some(device), nonces.clone(), 1_000);

        // 第一次：成功。
        let input1 = input_with_signature(
            FAKE_TOKEN_HEX,
            1_000,
            "nonce-x",
            "GET",
            "/x",
            FAKE_BODY_HASH_HEX,
        );
        uc.execute(input1).await.expect("first ok");

        // 第二次同 nonce：重放。
        let input2 = input_with_signature(
            FAKE_TOKEN_HEX,
            1_000,
            "nonce-x",
            "GET",
            "/x",
            FAKE_BODY_HASH_HEX,
        );
        let err = uc.execute(input2).await.unwrap_err();
        assert!(matches!(err, AuthenticateMobileRequestError::NonceReplay));
    }

    #[tokio::test]
    async fn nonce_cache_full_returns_dedicated_error() {
        let device = fake_device(token_hash_for(FAKE_TOKEN_HEX));
        let nonces = Arc::new(StubNonces {
            seen: Mutex::new(vec![]),
            force_full: true,
        });
        let uc = make_uc(Some(device), nonces, 1_000);
        let input =
            input_with_signature(FAKE_TOKEN_HEX, 1_000, "n1", "GET", "/x", FAKE_BODY_HASH_HEX);
        let err = uc.execute(input).await.unwrap_err();
        assert!(matches!(
            err,
            AuthenticateMobileRequestError::NonceCacheFull
        ));
    }

    #[tokio::test]
    async fn invalid_signature_rejected() {
        let device = fake_device(token_hash_for(FAKE_TOKEN_HEX));
        let nonces = Arc::new(StubNonces::default());
        let uc = make_uc(Some(device), nonces, 1_000);
        let mut input =
            input_with_signature(FAKE_TOKEN_HEX, 1_000, "n1", "GET", "/x", FAKE_BODY_HASH_HEX);
        // 篡改：随便改一个 hex 字符。
        let mut sig = input.headers.signature_hex.clone();
        let first = sig.remove(0);
        let new_first = if first == 'a' { '1' } else { 'a' };
        sig.insert(0, new_first);
        input.headers.signature_hex = sig;

        let err = uc.execute(input).await.unwrap_err();
        assert!(matches!(
            err,
            AuthenticateMobileRequestError::InvalidSignature
        ));
    }

    #[tokio::test]
    async fn invalid_token_format_rejected_before_lookup() {
        let nonces = Arc::new(StubNonces::default());
        let uc = make_uc(None, nonces, 1_000);
        let input = AuthenticateMobileRequestInput {
            headers: MobileAuthHeaders {
                token_hex: "not-hex".into(),
                timestamp_ms: 1_000,
                nonce: "n1".into(),
                signature_hex: FAKE_BODY_HASH_HEX.into(),
            },
            method: "GET".into(),
            path: "/x".into(),
            body_hash_hex: FAKE_BODY_HASH_HEX.into(),
        };
        let err = uc.execute(input).await.unwrap_err();
        assert!(matches!(
            err,
            AuthenticateMobileRequestError::InvalidTokenFormat
        ));
    }

    #[tokio::test]
    async fn empty_nonce_rejected() {
        let nonces = Arc::new(StubNonces::default());
        let uc = make_uc(None, nonces, 1_000);
        let input = AuthenticateMobileRequestInput {
            headers: MobileAuthHeaders {
                token_hex: FAKE_TOKEN_HEX.into(),
                timestamp_ms: 1_000,
                nonce: "".into(),
                signature_hex: FAKE_BODY_HASH_HEX.into(),
            },
            method: "GET".into(),
            path: "/x".into(),
            body_hash_hex: FAKE_BODY_HASH_HEX.into(),
        };
        let err = uc.execute(input).await.unwrap_err();
        assert!(matches!(
            err,
            AuthenticateMobileRequestError::InvalidNonceFormat
        ));
    }

    #[test]
    fn signature_canonical_string_matches_spec() {
        // 黄金值：手算 SHA-256("a\n1000\nn\nGET\n/x\nb")
        // 用代码算一次,验证签名公式不被悄悄改。
        let computed = compute_signature_hex("a", 1000, "n", "GET", "/x", "b");
        // 重新算一份用作"sanity check"——若拼接公式被改,这里也会一起变,
        // 所以再用 raw sha2 算一遍做交叉验证。
        let mut h = Sha256::new();
        h.update(b"a\n1000\nn\nGET\n/x\nb");
        let manual = hex::encode(h.finalize());
        assert_eq!(computed, manual);
    }

    #[test]
    fn ct_eq_handles_case_insensitivity() {
        assert!(ct_eq_ignore_case_hex("ABCD", "abcd"));
        assert!(!ct_eq_ignore_case_hex("ABCE", "abcd"));
        // 不同长度直接 false。
        assert!(!ct_eq_ignore_case_hex("abcd", "abcde"));
    }
}
