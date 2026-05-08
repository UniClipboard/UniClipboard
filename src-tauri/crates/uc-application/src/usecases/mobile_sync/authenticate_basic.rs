//! `AuthenticateBasicAuthUseCase` —— LAN HTTP 鉴权热路径。
//!
//! SyncClipboard shortcut 客户端在每次 `GET/PUT /SyncClipboard.json` /
//! `GET/PUT /file/:dataName` 请求里都带 `Authorization: basic
//! base64(user:pass)`(SyncClipboard 项目用小写 `basic`, RFC 对 scheme 不
//! 区分大小写, 我们这里也接受 Basic / BASIC)。本 use case 把请求头里的
//! 鉴权字符串翻成"是哪台已登记设备 + 是否合法"的业务事实, 路由层据此决
//! 定 200 / 401。
//!
//! 设计要点:
//!
//! 1. **不持有请求 / 响应类型**。入参就是裸 `Authorization` header 值
//!    (含 scheme 前缀), 出参是 `AuthenticatedDevice`(已绑定 device 实体)
//!    或 `AuthenticateBasicAuthError`。HTTP 适配是 webserver 的事, 这里
//!    只表达应用语义。
//! 2. **静默 401 通道**。所有"找不到设备 / 密码不对 / 头格式坏 /
//!    base64 坏"都翻译成同一类 `InvalidCredentials`, 不向外区分细节
//!    (避免给攻击者枚举哪种信息存在)。技术错误(repository 故障 / hasher
//!    PHC 损坏)单独走 `Storage` / `Internal` 让上层日志可见。
//! 3. **constant-time 比较**由 [`PasswordHasherPort::verify`] 内部保证;
//!    本 use case 不在外面做"先 username 比对再说"的提前短路, 让 hasher
//!    那 ~50ms 成为统一时长 ceiling, 哪怕命中"用户名不存在"也跑一次假
//!    验证(实现见下文)。
//! 4. **不更新 last_seen_*** —— 那是上层路由 happy path 后再决定是否调
//!    `record_activity` 的事(给路由更细的控制粒度: 401 不应当 last_seen,
//!    成功的请求才应当)。

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::Engine;
use tracing::instrument;

use uc_core::mobile_sync::{MobileDevice, MobileDeviceError};
use uc_core::ports::{MobileDeviceRepositoryPort, PasswordHasherError, PasswordHasherPort};

// ─── public-shaped (input / output / error) ─────────────────────────────

/// 调用方提交的请求:HTTP `Authorization` 头的原始值, 含 scheme 前缀,
/// 例如 `"basic bW9iaWxlXzAwMDE6cGFzcw=="`。
#[derive(Debug, Clone)]
pub struct AuthenticateBasicAuthInput {
    pub authorization_header: String,
}

/// 鉴权成功的产物:已被仓储确认存在并通过密码校验的 device。
///
/// 上层路由拿到它后, 通常会:
///   1. 把 `device` 塞进 axum extension 供后续 handler 用;
///   2. 调 facade 的 record_activity 异步更新 last_seen_*。
#[derive(Debug, Clone)]
pub struct AuthenticatedDevice {
    pub device: MobileDevice,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthenticateBasicAuthError {
    /// 401 通道 —— 头缺失 / scheme 不对 / base64 / 格式坏 / 用户名不存在 /
    /// 密码不对, 一律视为同一种"凭据无效"对外。
    #[error("invalid credentials")]
    InvalidCredentials,

    /// 仓储读失败 —— 应允许重试, 与"凭据无效"语义不同。
    #[error("device persistence failed: {0}")]
    PersistenceFailed(String),

    /// 密码哈希器内部错误(库故障 / spawn_blocking join 失败)。
    /// PHC 字符串本身损坏(字段被人手改坏)按 401 处理而不是 Internal,
    /// 避免攻击者通过制造畸形 phc 字段触发服务侧错误日志风暴。
    #[error("password hasher internal failure: {0}")]
    Internal(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct AuthenticateBasicAuthUseCase {
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    password_hasher: Arc<dyn PasswordHasherPort>,
}

impl AuthenticateBasicAuthUseCase {
    pub(crate) fn new(
        device_repo: Arc<dyn MobileDeviceRepositoryPort>,
        password_hasher: Arc<dyn PasswordHasherPort>,
    ) -> Self {
        Self {
            device_repo,
            password_hasher,
        }
    }

    /// 校验 Basic Auth 头, 命中即返回对应 device。
    ///
    /// 时间侧信道防御:无论用户名是否存在, 都会跑一次密码 verify(命中
    /// 时用真实 PHC, 未命中时用一段已知 PHC), 让"用户名存在"和"用户名
    /// 不存在"两条路径耗时一致(约一次 Argon2id verify 的时长)。
    #[instrument(skip(self, input), fields(header_len = input.authorization_header.len()))]
    pub(crate) async fn execute(
        &self,
        input: AuthenticateBasicAuthInput,
    ) -> Result<AuthenticatedDevice, AuthenticateBasicAuthError> {
        // 1. 解析头 -> (username, password)。任何步骤失败 → 401。
        let (username, password) = match parse_basic_header(&input.authorization_header) {
            Some(pair) => pair,
            None => {
                self.run_dummy_verify().await;
                return Err(AuthenticateBasicAuthError::InvalidCredentials);
            }
        };

        // 2. 查仓储。
        let found = self
            .device_repo
            .find_by_username(&username)
            .await
            .map_err(translate_device_error)?;

        // 3. 跑一次 verify, 命中 / 未命中走相同长度的 CPU 工作。
        let device = match found {
            Some(device) => {
                let phc = device.password_hash.clone();
                match self.password_hasher.verify(&password, &phc).await {
                    Ok(true) => device,
                    Ok(false) => return Err(AuthenticateBasicAuthError::InvalidCredentials),
                    Err(PasswordHasherError::InvalidPhc(_)) => {
                        // PHC 本身损坏 —— 当作 401, 不暴露给攻击者细节。
                        // 不算 Internal: 仓储里这条记录已坏, 用户重新登记
                        // 即可解决, 不该让 caller 重试当前请求。
                        return Err(AuthenticateBasicAuthError::InvalidCredentials);
                    }
                    Err(PasswordHasherError::Internal(msg)) => {
                        return Err(AuthenticateBasicAuthError::Internal(msg));
                    }
                }
            }
            None => {
                // 用户名不存在 —— 仍跑一次 verify 让耗时一致, 然后返回 401。
                self.run_dummy_verify().await;
                return Err(AuthenticateBasicAuthError::InvalidCredentials);
            }
        };

        Ok(AuthenticatedDevice { device })
    }

    /// 跑一次 dummy verify, 让"用户名不存在"路径与"用户名存在但密码错"
    /// 路径耗时一致。dummy_phc 是一段固定的(用 invalid scheme 字符串触发
    /// InvalidPhc, 不会跑真正的 argon2 计算)还是真 phc?这里选**真 PHC**,
    /// 让两条路径都触发同一档 Argon2id 计算, 真正 constant-time。
    async fn run_dummy_verify(&self) {
        // 这条 PHC 是固定参数 + 任意 hash bytes 的合法 Argon2id PHC ——
        // 用 "x" 作 password 跑 verify 会真实跑一次 Argon2 计算, 不命中
        // 但耗时与命中失败一致。生成方式: 用 OsRngCredentialsMinter 颁发
        // 一次性产物里取 password_hash, 然后写死在这里。
        const DUMMY_PHC: &str = "$argon2id$v=19$m=65536,t=3,p=4$AAAAAAAAAAAAAAAAAAAAAA$+x46P28S/o\
             8eL5Yzb9SRvfGFIYRkBQVj4lO2Wx9LO50";
        // 忽略所有错误 —— dummy 路径任何失败都不该影响调用方语义,
        // 调用方拿到的本来就是 401。
        let _ = self
            .password_hasher
            .verify("dummy-password", DUMMY_PHC)
            .await;
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

/// 把 `"basic <base64>"` / `"Basic <base64>"` / `"BASIC <base64>"` 拆成
/// `(username, password)`。任何失败返回 `None`(由 caller 翻 401)。
///
/// 安全要点:
/// - scheme 比较忽略大小写(SyncClipboard 项目用小写, RFC 7617 不区分);
/// - base64 走标准 alphabet, 不带 padding 都接受不了(SyncClipboard
///   shortcut 编码出来一定带 `=`);
/// - 解码出的 bytes 必须是 UTF-8 才能正常拆 `:`。
fn parse_basic_header(header: &str) -> Option<(String, String)> {
    let trimmed = header.trim();
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let scheme = parts.next()?;
    let payload = parts.next()?.trim();
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = BASE64_STD.decode(payload).ok()?;
    let decoded_str = std::str::from_utf8(&decoded).ok()?;
    let mut split = decoded_str.splitn(2, ':');
    let username = split.next()?.to_string();
    let password = split.next()?.to_string();
    if username.is_empty() {
        return None;
    }
    Some((username, password))
}

fn translate_device_error(err: MobileDeviceError) -> AuthenticateBasicAuthError {
    match err {
        MobileDeviceError::Storage(msg) => AuthenticateBasicAuthError::PersistenceFailed(msg),
        // find_by_username 不会触发 AlreadyExists / UsernameCollision;
        // 走到这里说明 adapter 违约, 兜底为 PersistenceFailed。
        other => AuthenticateBasicAuthError::PersistenceFailed(other.to_string()),
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;

    use uc_core::mobile_sync::{MobileClientType, MobileDeviceId};

    mockall::mock! {
        DeviceRepo {}
        #[async_trait]
        impl MobileDeviceRepositoryPort for DeviceRepo {
            async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError>;
            async fn find_by_username(
                &self,
                username: &str,
            ) -> Result<Option<MobileDevice>, MobileDeviceError>;
            async fn find_by_device_id(
                &self,
                device_id: &MobileDeviceId,
            ) -> Result<Option<MobileDevice>, MobileDeviceError>;
            async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError>;
            async fn delete(&self, device_id: &MobileDeviceId) -> Result<bool, MobileDeviceError>;
            async fn record_activity(
                &self,
                device_id: &MobileDeviceId,
                last_seen_at_ms: i64,
                last_seen_ip: Option<String>,
                reported_name: Option<String>,
                reported_os: Option<String>,
            ) -> Result<(), MobileDeviceError>;
            async fn update_password_hash(
                &self,
                device_id: &MobileDeviceId,
                new_password_hash: String,
            ) -> Result<bool, MobileDeviceError>;
        }
    }

    mockall::mock! {
        Hasher {}
        #[async_trait]
        impl PasswordHasherPort for Hasher {
            async fn hash(&self, password: &str) -> Result<String, PasswordHasherError>;
            async fn verify(&self, password: &str, phc: &str)
                -> Result<bool, PasswordHasherError>;
        }
    }

    fn make_device(username: &str, phc: &str) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new("did_test"),
            label: "iPhone".into(),
            client_type: MobileClientType::IosShortcut,
            username: username.into(),
            password_hash: phc.into(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        }
    }

    /// 把 device 列表包装成 mock repo —— find_by_username 按 username 命中
    /// 返回对应 device, 找不到返回 None。其它方法不设 expectation, 一旦被
    /// 调用 mockall 会自动 panic。
    fn repo_with(devices: Vec<MobileDevice>) -> MockDeviceRepo {
        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_username().returning(move |username| {
            Ok(devices.iter().find(|d| d.username == username).cloned())
        });
        repo
    }

    /// "phc:<plain>" 形态的伪 hasher —— 不真跑 Argon2,但表达"密码与 PHC
    /// 是否对应"的语义。verify 时 PHC == `format!("phc:{password}")` 即视
    /// 为命中。
    fn fake_hasher() -> MockHasher {
        let mut h = MockHasher::new();
        h.expect_verify().returning(|password, phc| {
            if phc == "PHC_BROKEN" {
                return Err(PasswordHasherError::InvalidPhc("malformed".into()));
            }
            Ok(phc == format!("phc:{password}"))
        });
        h
    }

    fn build_uc(devices: Vec<MobileDevice>) -> AuthenticateBasicAuthUseCase {
        AuthenticateBasicAuthUseCase::new(Arc::new(repo_with(devices)), Arc::new(fake_hasher()))
    }

    fn header_for(username: &str, password: &str) -> String {
        let payload = BASE64_STD.encode(format!("{username}:{password}"));
        format!("basic {payload}")
    }

    #[tokio::test]
    async fn happy_path_returns_device() {
        let device = make_device("mobile_aaaa", "phc:hunter2");
        let uc = build_uc(vec![device.clone()]);
        let out = uc
            .execute(AuthenticateBasicAuthInput {
                authorization_header: header_for("mobile_aaaa", "hunter2"),
            })
            .await
            .expect("ok");
        assert_eq!(out.device.username, "mobile_aaaa");
    }

    #[tokio::test]
    async fn accepts_capitalized_basic_scheme() {
        let device = make_device("mobile_aaaa", "phc:hunter2");
        let uc = build_uc(vec![device]);
        let payload = BASE64_STD.encode("mobile_aaaa:hunter2");
        for scheme in ["Basic", "BASIC", "BaSiC"] {
            let header = format!("{scheme} {payload}");
            uc.execute(AuthenticateBasicAuthInput {
                authorization_header: header,
            })
            .await
            .expect("scheme case must be ignored");
        }
    }

    #[tokio::test]
    async fn rejects_unknown_username() {
        let uc = build_uc(vec![]);
        let err = uc
            .execute(AuthenticateBasicAuthInput {
                authorization_header: header_for("mobile_ghost", "anything"),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AuthenticateBasicAuthError::InvalidCredentials
        ));
    }

    #[tokio::test]
    async fn rejects_wrong_password() {
        let device = make_device("mobile_aaaa", "phc:rightpw");
        let uc = build_uc(vec![device]);
        let err = uc
            .execute(AuthenticateBasicAuthInput {
                authorization_header: header_for("mobile_aaaa", "wrongpw"),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AuthenticateBasicAuthError::InvalidCredentials
        ));
    }

    #[tokio::test]
    async fn rejects_malformed_header() {
        // 头解析失败时 use case 仍跑一次 dummy verify(防侧信道),所以
        // hasher.verify 必须能被调用 —— 用 fake_hasher() 即可(任意输入
        // 都返 Ok(false))。仓储**永远不该**在头坏时被查,断言 never。
        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_username().never();
        let uc = AuthenticateBasicAuthUseCase::new(Arc::new(repo), Arc::new(fake_hasher()));

        for bad in [
            "",
            "basic",
            "bearer foo",
            "basic notbase64!!!",
            "basic Zm9v", // base64 of "foo" — no colon
        ] {
            let err = uc
                .execute(AuthenticateBasicAuthInput {
                    authorization_header: bad.into(),
                })
                .await
                .unwrap_err();
            assert!(
                matches!(err, AuthenticateBasicAuthError::InvalidCredentials),
                "bad header {bad:?} should yield InvalidCredentials"
            );
        }
    }

    #[tokio::test]
    async fn rejects_corrupt_phc_as_invalid_credentials() {
        // PHC 字符串损坏不暴露给攻击者 —— 走 401, 不走 Internal。
        let device = make_device("mobile_aaaa", "PHC_BROKEN");
        let uc = build_uc(vec![device]);
        let err = uc
            .execute(AuthenticateBasicAuthInput {
                authorization_header: header_for("mobile_aaaa", "anything"),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AuthenticateBasicAuthError::InvalidCredentials
        ));
    }

    #[tokio::test]
    async fn translates_storage_error() {
        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_username()
            .returning(|_| Err(MobileDeviceError::Storage("disk gone".into())));
        let uc = AuthenticateBasicAuthUseCase::new(Arc::new(repo), Arc::new(fake_hasher()));

        let err = uc
            .execute(AuthenticateBasicAuthInput {
                authorization_header: header_for("mobile_aaaa", "x"),
            })
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthenticateBasicAuthError::PersistenceFailed(ref s) if s.contains("disk gone")),
            "expected PersistenceFailed, got {err:?}"
        );
    }

    #[tokio::test]
    async fn translates_hasher_internal_error() {
        let device = make_device("mobile_aaaa", "phc:hunter2");
        let mut hasher = MockHasher::new();
        hasher
            .expect_verify()
            .returning(|_, _| Err(PasswordHasherError::Internal("forced".into())));

        let uc =
            AuthenticateBasicAuthUseCase::new(Arc::new(repo_with(vec![device])), Arc::new(hasher));
        let err = uc
            .execute(AuthenticateBasicAuthInput {
                authorization_header: header_for("mobile_aaaa", "hunter2"),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthenticateBasicAuthError::Internal(_)));
    }
}
