//! `RotateMobilePasswordUseCase` —— 给一台已登记设备换一份新密码。
//!
//! ## 为什么单独一个 use case 而不是复用 register
//!
//! 注册和轮换在用户视角是两件事:注册产出新设备身份(device_id / username
//! / 凭据 + install URL + QR);轮换只换"凭据"中的密码,设备身份不变。
//! 强行复用 register 会把 LAN 探测 / QR 渲染 / settings 校验拖进来,而轮换
//! 路径其实只关心 device_repo + password_hasher + minter 三个端口。
//!
//! ## 流程
//!
//! 1. `find_by_device_id` 查到目标设备 → 拿到稳定的 `username` 与其它字段
//!    (`NotFound` 直接返回)。
//! 2. 决定新明文:`input.password = Some(p)` → 校验长度;`None` → 走 minter
//!    取新明文(只用 `MintedCredentials.password`,**不**取 username /
//!    device_id —— 这台设备已经有自己的稳定身份)。
//! 3. `password_hasher.hash(new_plaintext)` → 拿到新 PHC。
//! 4. `device_repo.update_password_hash(device_id, new_phc)`:`Ok(true)` 表示
//!    成功;`Ok(false)` 表示设备在我们 read-then-write 之间被并发撤销 ——
//!    翻成 `NotFound`,与 step 1 的语义保持一致。
//! 5. 返回 `(device_id, username, new_plaintext)` —— 明文是**唯一一次**面向
//!    用户回显, 之后只以 PHC 形式存在。
//!
//! ## 安全语义
//!
//! 轮换成功后,旧密码立即失效:任何还在用旧密码的 iPhone shortcut 下次请求
//! 必收 401。这不是 use case 主动做的事 —— 是 `authenticate_basic` 路径在
//! 鉴权时永远 verify 当前 PHC,所以 PHC 一换,旧密码自动作废。
//!
//! 不验证旧密码:这是设备主自己的桌面端在轮换自己的设备凭据,信任边界
//! 在桌面端(daemon 进程主),不在 LAN 端。如果有人能通过 daemon 调用本
//! use case,他已经有比"知道旧密码"更高的权限。

use std::sync::Arc;

use tracing::{debug, instrument};

use uc_core::mobile_sync::{MintedCredentials, MobileDeviceError, MobileDeviceId};
use uc_core::ports::{
    MobileCredentialsMinterPort, MobileDeviceRepositoryPort, PasswordHasherError,
    PasswordHasherPort,
};

use super::register_device::{MAX_PASSWORD_LEN, MIN_PASSWORD_LEN};

// ─── public-shaped (input / output / error) ─────────────────────────────

/// 调用方提交的请求。`password = None` 走 minter 自动颁发新明文;`Some(p)`
/// 走自定义路径,本 use case 校验长度。
#[derive(Debug, Clone)]
pub struct RotateMobilePasswordInput {
    pub device_id: MobileDeviceId,
    pub password: Option<String>,
}

/// 轮换成功的产物。`password` 是**唯一一次**明文回显 —— 之后只以
/// PHC 形式存在。前端必须立即在 modal 里展示给用户,并提示"旧密码已失
/// 效, 请同步更新 iPhone shortcut 里的 password 字段"。
#[derive(Debug, Clone)]
pub struct RotateMobilePasswordOutput {
    pub device_id: MobileDeviceId,
    /// 设备登录账号(轮换不变)。一并返回方便前端展示"账号 + 新密码"
    /// 而无需再查一次设备。
    pub username: String,
    pub password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RotateMobilePasswordError {
    /// 目标设备不存在 —— 用户已撤销 / UI 列表过期 / 并发竞争。
    #[error("device not found: {0}")]
    NotFound(MobileDeviceId),

    /// 自定义 password 长度低于 [`MIN_PASSWORD_LEN`]。
    #[error("password too short (min {min} chars)")]
    PasswordTooShort { min: usize },

    /// 自定义 password 长度超过 [`MAX_PASSWORD_LEN`]。Argon2id DOS 防护。
    #[error("password too long (max {max} chars)")]
    PasswordTooLong { max: usize },

    /// 哈希失败(算法库内部错误)。
    #[error("password hashing failed: {0}")]
    PasswordHashFailed(String),

    /// 持久化失败(底层存储错误)。
    #[error("device persistence failed: {0}")]
    PersistenceFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct RotateMobilePasswordUseCase {
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    password_hasher: Arc<dyn PasswordHasherPort>,
    credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
}

impl RotateMobilePasswordUseCase {
    pub(crate) fn new(
        device_repo: Arc<dyn MobileDeviceRepositoryPort>,
        password_hasher: Arc<dyn PasswordHasherPort>,
        credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
    ) -> Self {
        Self {
            device_repo,
            password_hasher,
            credentials_minter,
        }
    }

    #[instrument(skip(self, input), fields(custom_password = input.password.is_some()))]
    pub(crate) async fn execute(
        &self,
        input: RotateMobilePasswordInput,
    ) -> Result<RotateMobilePasswordOutput, RotateMobilePasswordError> {
        // 1. 查目标设备 —— 拿到稳定的 username。
        let device = self
            .device_repo
            .find_by_device_id(&input.device_id)
            .await
            .map_err(translate_device_error)?
            .ok_or_else(|| RotateMobilePasswordError::NotFound(input.device_id.clone()))?;

        // 2. 决定新明文。
        let new_password = match input.password {
            Some(p) => {
                validate_password_length(&p)?;
                p
            }
            None => {
                let MintedCredentials { password, .. } = self.credentials_minter.mint_credentials();
                password
            }
        };

        // 3. hash → PHC
        let new_phc = self
            .password_hasher
            .hash(&new_password)
            .await
            .map_err(translate_hasher_error)?;

        // 4. 更新仓储。Ok(false) → 设备在 read-then-write 之间被撤销,翻
        //    成 NotFound 让 UI 提示刷新。
        let updated = self
            .device_repo
            .update_password_hash(&device.device_id, new_phc)
            .await
            .map_err(translate_device_error)?;
        if !updated {
            debug!(
                device_id = %device.device_id,
                "device disappeared between find and update_password_hash (concurrent revoke)"
            );
            return Err(RotateMobilePasswordError::NotFound(device.device_id));
        }

        // 5. 一次性回显新明文。
        Ok(RotateMobilePasswordOutput {
            device_id: device.device_id,
            username: device.username,
            password: new_password,
        })
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

fn validate_password_length(password: &str) -> Result<(), RotateMobilePasswordError> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err(RotateMobilePasswordError::PasswordTooShort {
            min: MIN_PASSWORD_LEN,
        });
    }
    if len > MAX_PASSWORD_LEN {
        return Err(RotateMobilePasswordError::PasswordTooLong {
            max: MAX_PASSWORD_LEN,
        });
    }
    Ok(())
}

fn translate_device_error(err: MobileDeviceError) -> RotateMobilePasswordError {
    match err {
        MobileDeviceError::Storage(msg) => RotateMobilePasswordError::PersistenceFailed(msg),
        // rotate 路径不应触发 AlreadyExists / UsernameCollision —— 走到这里
        // 说明 adapter 实现违反契约, 兜底翻成 PersistenceFailed。
        other => RotateMobilePasswordError::PersistenceFailed(other.to_string()),
    }
}

fn translate_hasher_error(err: PasswordHasherError) -> RotateMobilePasswordError {
    match err {
        PasswordHasherError::InvalidPhc(msg) => {
            RotateMobilePasswordError::PasswordHashFailed(format!("invalid phc: {msg}"))
        }
        PasswordHasherError::Internal(msg) => RotateMobilePasswordError::PasswordHashFailed(msg),
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! mockall 单测:三个 ports(device repo / hasher / minter)各自 mock,
    //! 用 expectations 表达"这次调用应该被传什么参数 / 应该返回什么"。
    //! 与手写 fixture 相比,断言入参的 .with(eq(...)) 让"use case 真的把
    //! 正确的值送下去了"也变成可观测的 —— 比如 update_password_hash 必须
    //! 收到 hasher 算出的那串 PHC,而不是其它东西。
    use super::*;

    use mockall::predicate::eq;

    use uc_core::mobile_sync::{MobileClientType, MobileDevice};

    // DeviceRepo / Hasher / Minter mock 在 mobile_sync 多处复用,集中
    // 在 test_support 模块。
    use super::super::test_support::{MockDeviceRepo, MockHasher, MockMinter};

    // ── helpers ────────────────────────────────────────────────────────

    fn fixture_device(id: &str, username: &str) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new(id),
            label: "phone".into(),
            client_type: MobileClientType::IosShortcut,
            username: username.into(),
            password_hash: "phc:OLD".into(),
            created_at_ms: 1_000,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        }
    }

    /// 大多数测试只用前两个 mock + 一个 minter,这里把 minter 的"产出哪个
    /// 明文"参数化,让每个测试自由选择(rotate path 只用 minter.password,
    /// 其它字段是无关项,统一填 placeholder)。
    fn minter_emitting(password: &'static str) -> MockMinter {
        let mut m = MockMinter::new();
        m.expect_mint_credentials()
            .returning(move || MintedCredentials {
                username: "mobile_unused".into(),
                password: password.into(),
                password_hash: "$argon2id$test$minted".into(),
                device_id: MobileDeviceId::new("did_unused"),
            });
        m
    }

    /// 钩入"identity-like"hasher:输出 `phc:<plaintext>`。配合 .with(eq(..))
    /// 就能在 update_password_hash 的入参上断言"hasher 算出的 PHC 真的被
    /// 送进了仓储",这是手写 fixture 表达不出来的。
    fn identity_hasher_for(plaintext: &'static str) -> MockHasher {
        let mut h = MockHasher::new();
        h.expect_hash()
            .with(eq(plaintext))
            .returning(|p| Ok(format!("phc:{p}")));
        h
    }

    // ── tests ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn rotates_to_minted_password_when_input_is_none() {
        let device = fixture_device("did_x", "mobile_alice");
        let device_id = device.device_id.clone();

        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id()
            .with(eq(device_id.clone()))
            .returning({
                let device = device.clone();
                move |_| Ok(Some(device.clone()))
            });
        repo.expect_update_password_hash()
            .with(
                eq(device_id.clone()),
                eq("phc:minted-rotate-pw-22".to_string()),
            )
            .returning(|_, _| Ok(true));

        let uc = RotateMobilePasswordUseCase::new(
            Arc::new(repo),
            Arc::new(identity_hasher_for("minted-rotate-pw-22")),
            Arc::new(minter_emitting("minted-rotate-pw-22")),
        );

        let out = uc
            .execute(RotateMobilePasswordInput {
                device_id: device_id.clone(),
                password: None,
            })
            .await
            .expect("ok");

        assert_eq!(out.password, "minted-rotate-pw-22");
        assert_eq!(out.username, "mobile_alice");
        assert_eq!(out.device_id, device_id);
    }

    #[tokio::test]
    async fn rotates_to_custom_password_when_provided() {
        let device = fixture_device("did_x", "mobile_alice");
        let device_id = device.device_id.clone();

        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        repo.expect_update_password_hash()
            .with(
                eq(device_id.clone()),
                eq("phc:brand-new-pass-42".to_string()),
            )
            .returning(|_, _| Ok(true));

        // minter 不应被调用 —— 给一个会让 mockall 在意外触发时报错的 mock
        let mut minter = MockMinter::new();
        minter.expect_mint_credentials().never();

        let uc = RotateMobilePasswordUseCase::new(
            Arc::new(repo),
            Arc::new(identity_hasher_for("brand-new-pass-42")),
            Arc::new(minter),
        );

        let out = uc
            .execute(RotateMobilePasswordInput {
                device_id,
                password: Some("brand-new-pass-42".into()),
            })
            .await
            .expect("ok");

        assert_eq!(out.password, "brand-new-pass-42");
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_device() {
        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning(|_| Ok(None));
        // 找不到设备就不应再走 hasher / update_password_hash,断言一下没被调用
        repo.expect_update_password_hash().never();
        let mut hasher = MockHasher::new();
        hasher.expect_hash().never();

        let uc = RotateMobilePasswordUseCase::new(
            Arc::new(repo),
            Arc::new(hasher),
            Arc::new(MockMinter::new()),
        );

        let err = uc
            .execute(RotateMobilePasswordInput {
                device_id: MobileDeviceId::new("did_ghost"),
                password: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, RotateMobilePasswordError::NotFound(_)));
    }

    #[tokio::test]
    async fn returns_not_found_when_concurrent_revoke_between_find_and_update() {
        // find 返回 Some, 但 update_password_hash 返回 false (并发撤销)。
        // use case 必须翻成 NotFound, 而不是把它吞掉。
        let device = fixture_device("did_x", "mobile_alice");
        let device_id = device.device_id.clone();

        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        repo.expect_update_password_hash()
            .returning(|_, _| Ok(false));

        let uc = RotateMobilePasswordUseCase::new(
            Arc::new(repo),
            Arc::new(identity_hasher_for("minted-rotate-pw-22")),
            Arc::new(minter_emitting("minted-rotate-pw-22")),
        );

        let err = uc
            .execute(RotateMobilePasswordInput {
                device_id,
                password: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, RotateMobilePasswordError::NotFound(_)));
    }

    #[tokio::test]
    async fn rejects_password_too_short() {
        // 长度校验在 find 之后, 仓库 / hasher 不应被任何方式调用 —— 让两者
        // 都拿不到 expectation, 一旦被调用 mockall 会自动 panic。
        let device = fixture_device("did_x", "mobile_alice");
        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        repo.expect_update_password_hash().never();
        let mut hasher = MockHasher::new();
        hasher.expect_hash().never();

        let uc = RotateMobilePasswordUseCase::new(
            Arc::new(repo),
            Arc::new(hasher),
            Arc::new(MockMinter::new()),
        );

        let err = uc
            .execute(RotateMobilePasswordInput {
                device_id: device.device_id,
                password: Some("a".repeat(MIN_PASSWORD_LEN - 1)),
            })
            .await
            .unwrap_err();
        match err {
            RotateMobilePasswordError::PasswordTooShort { min } => {
                assert_eq!(min, MIN_PASSWORD_LEN)
            }
            other => panic!("expected PasswordTooShort, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejects_password_too_long() {
        let device = fixture_device("did_x", "mobile_alice");
        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        repo.expect_update_password_hash().never();
        let mut hasher = MockHasher::new();
        hasher.expect_hash().never();

        let uc = RotateMobilePasswordUseCase::new(
            Arc::new(repo),
            Arc::new(hasher),
            Arc::new(MockMinter::new()),
        );

        let err = uc
            .execute(RotateMobilePasswordInput {
                device_id: device.device_id,
                password: Some("a".repeat(MAX_PASSWORD_LEN + 1)),
            })
            .await
            .unwrap_err();
        match err {
            RotateMobilePasswordError::PasswordTooLong { max } => assert_eq!(max, MAX_PASSWORD_LEN),
            other => panic!("expected PasswordTooLong, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn translates_hasher_internal_error() {
        let device = fixture_device("did_x", "mobile_alice");
        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        // hasher 失败 → 不该走到 update
        repo.expect_update_password_hash().never();

        let mut hasher = MockHasher::new();
        hasher
            .expect_hash()
            .returning(|_| Err(PasswordHasherError::Internal("simulated".into())));

        let uc = RotateMobilePasswordUseCase::new(
            Arc::new(repo),
            Arc::new(hasher),
            Arc::new(MockMinter::new()),
        );

        let err = uc
            .execute(RotateMobilePasswordInput {
                device_id: device.device_id,
                password: Some("some-strong-pass".into()),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RotateMobilePasswordError::PasswordHashFailed(_)
        ));
    }
}
