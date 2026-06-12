//! 空间访问 port。
//!
//! 以"用户口令视角"把空间的初始化 / 解锁 / 换口令 / 加锁 /
//! 就绪查询合并到一个业务动作 port。签名里只出现领域中性类型
//! （`Passphrase` / `ActiveSpace`），密钥物料（KEK / MasterKey /
//! KeySlot / WrappedMasterKey 等）一律在 adapter 内部生成和持有，
//! 不穿过这一层。
//!
//! 合并了原先的 `EncryptionPort`（KDF + wrap/unwrap 部分）、
//! `EncryptionSessionPort`、`KeyMaterialPort`、以及已删除的 `space::CryptoPort`
//! 的三个方法（`generate_nonce32 / export_keyslot_blob / derive_master_key_from_keyslot`）。

use async_trait::async_trait;

use crate::crypto::domain::{ActiveSpace, Passphrase};
use crate::ids::SpaceId;
use crate::space_access::{JoinOffer, ProofDerivedKey};

/// 业务语义级的空间访问失败。
///
/// 故意不带密码学细节（algo / nonce / tag / blob 偏移等）——
/// adapter 内部可以保留更细的错误，这里只暴露调用方需要分支处理的类别。
#[derive(Debug, thiserror::Error)]
pub enum SpaceAccessError {
    /// 空间尚未初始化，无法解锁或换口令。
    #[error("space not initialized")]
    NotInitialized,

    /// 空间已初始化，不能再次 `initialize`。
    #[error("space already initialized")]
    AlreadyInitialized,

    /// 口令不匹配——unlock 时校验失败。
    #[error("wrong passphrase")]
    WrongPassphrase,

    /// 空间当前未解锁，无法执行需要会话的操作。
    #[error("space not unlocked")]
    NotUnlocked,

    /// 持久化的密钥物料损坏或版本不支持——属于数据层故障，不可恢复。
    #[error("space key material corrupted or unsupported")]
    CorruptedKeyMaterial,

    /// 其它内部故障（底层 IO / 算法实现异常等）。
    #[error("space access internal error: {0}")]
    Internal(String),
}

/// 空间访问 port。
///
/// 所有方法都以"空间"为中心——adapter 需要把 `SpaceId` 作为
/// 内部会话/密钥物料查找的键，外部调用方不感知 KEK / MasterKey。
#[async_trait]
pub trait SpaceAccessPort: Send + Sync {
    /// 首次初始化一个空间。
    ///
    /// 语义：
    /// - 生成全新的密钥物料并用口令派生的 KEK 加以包装，持久化落盘。
    /// - 完成后内存会话进入"已解锁"状态，返回 `ActiveSpace` 作为凭据。
    /// - 若该空间已初始化过，应返回 [`SpaceAccessError::AlreadyInitialized`]。
    async fn initialize(
        &self,
        space_id: &SpaceId,
        passphrase: &Passphrase,
    ) -> Result<ActiveSpace, SpaceAccessError>;

    /// 用口令解锁一个已初始化的空间。
    ///
    /// 语义：
    /// - 从持久化中读出包装后的密钥物料，用 `passphrase` 解包。
    /// - 成功后内存会话持有解包后的密钥，返回 `ActiveSpace`。
    /// - 口令错误返回 [`SpaceAccessError::WrongPassphrase`]。
    /// - 空间未初始化返回 [`SpaceAccessError::NotInitialized`]。
    async fn unlock(
        &self,
        space_id: &SpaceId,
        passphrase: &Passphrase,
    ) -> Result<ActiveSpace, SpaceAccessError>;

    /// 查询当前是否已解锁。
    async fn is_unlocked(&self, space_id: &SpaceId) -> bool;

    /// 清除内存会话——持久化密钥物料不受影响,后续仍可 `unlock`。
    async fn lock(&self, space_id: &SpaceId) -> Result<(), SpaceAccessError>;

    /// 工厂重置: 删除指定空间的全部持久化密钥物料 (磁盘 keyslot + keyring KEK)
    /// 并清空内存会话。`encryption_state` 持久化标记由调用方单独处理
    /// (avoid 让本 port 跨越 EncryptionStatePort 边界)。
    ///
    /// 用途: setup 流程"重置"操作 / 测试清理。删除"keyslot 不存在"或
    /// "KEK 不存在"等情况视作幂等成功,不报错。
    async fn factory_reset(&self, space_id: &SpaceId) -> Result<(), SpaceAccessError>;

    /// 静默尝试从持久化层（keyring 缓存）恢复会话——startup 路径专用。
    ///
    /// 行为：
    /// - 空间从未初始化：返回 `Ok(None)`（不视为错误,调用方据此判断是否需要引导用户首次 setup）；
    /// - 已初始化且 keyring 命中：解锁成功,返回 `Ok(Some(ActiveSpace))`；
    /// - keyring 缓存丢失 / 权限不足 / 密钥物料损坏：返回相应 [`SpaceAccessError`]。
    ///
    /// 与 `unlock` 的区别：本方法**不接受口令**,完全依赖 keyring 缓存——
    /// 适合 startup 静默恢复;若 keyring 失效（如用户清除 / 跨设备导入）,
    /// 调用方应回退到带口令的 `unlock` 路径。
    async fn try_resume_session(
        &self,
        space_id: &SpaceId,
    ) -> Result<Option<ActiveSpace>, SpaceAccessError>;

    /// 探测 keyring 当前是否能在静默下读出本空间的 KEK。
    ///
    /// 用途：macOS Keychain "Always Allow" 引导流程——首次访问会弹权限
    /// 提示框,用户授予 "Always Allow" 后再次调用应静默成功。
    ///
    /// 行为：
    /// - `Ok(true)`：keyring 静默命中,无需用户授权；
    /// - `Ok(false)`：权限被拒绝 / keyring 暂时不可用——调用方应当作"未授予 Always Allow"对待；
    /// - `Err(NotInitialized)`：本空间从未初始化,keyring 里没有 KEK;
    /// - `Err(Internal)`：其他不可恢复错误。
    async fn verify_keychain_access(&self) -> Result<bool, SpaceAccessError>;

    /// 从当前会话派生 32 字节子密钥（HKDF-SHA256,IKM = MasterKey）。
    ///
    /// 调用方决定 `salt`（一般是某种业务作用域,如 profile_id）和 `info`
    /// （区分用途的字符串,如 `"uniclipboard-search-index/v1"`）。adapter
    /// 内部用 IKM = master_key 派生,不暴露 master_key 字节。
    ///
    /// 用途：搜索索引 SearchKey 派生、未来其它需要派生密钥的场景。
    /// 会话未解锁时返回 [`SpaceAccessError::NotUnlocked`]。
    async fn derive_subkey(&self, salt: &[u8], info: &[u8]) -> Result<[u8; 32], SpaceAccessError>;

    /// 取出当前已解锁会话的 proof 链路凭据(包装的 master_key 字节)。
    ///
    /// 仅供 sponsor 侧 `ProofPort::verify_proof` 在 cache miss
    /// （进程重启 / cache 失效）时重新计算 HMAC——它需要拿到
    /// build_proof 时使用的同一份字节。
    ///
    /// 行为:
    /// - 会话已解锁: `Ok(Some(ProofDerivedKey))`
    /// - 会话未解锁: `Ok(None)` (调用方应当作 verify 失败处理)
    /// - 内部 IO 失败: `Err(Internal)`
    ///
    /// 注: 这是"读取已解锁会话的 master_key 字节"的窄接口,与 joiner 侧的
    /// `derive_master_key_for_proof` (从 JoinOffer + Passphrase 派生) 形成对称。
    async fn current_session_proof_key(&self) -> Result<Option<ProofDerivedKey>, SpaceAccessError>;

    /// Sponsor 侧：准备 pairing offer。
    ///
    /// 读取/生成该空间的 keyslot 序列化字节 + 产生 32 字节挑战 nonce,打包给 joiner。
    ///
    /// 注：签名保留 `passphrase` 参数以忠实反映当前 sponsor 侧"准备 offer =
    /// 顺带首次初始化（若未初始化）"的行为。若未来拆分"已初始化 sponsor 只读
    /// offer"vs"首次初始化 + 建 offer"两种语义,应在独立的清理阶段进行。
    async fn prepare_join_offer(
        &self,
        space_id: &SpaceId,
        passphrase: &Passphrase,
    ) -> Result<JoinOffer, SpaceAccessError>;

    /// Joiner 侧：用口令解开 offer 的 keyslot 字节,派生出构造 proof 所需的不透明凭据。
    ///
    /// 返回的 `ProofDerivedKey` 是只在本次 pairing proof 链路里有意义的
    /// 32 字节秘密——后续直接喂给 `ProofPort::build_proof` 计算 HMAC,
    /// 领域代码无需也无法把它当作 `MasterKey` 转用到其它路径。
    async fn derive_master_key_for_proof(
        &self,
        offer: &JoinOffer,
        passphrase: &Passphrase,
    ) -> Result<ProofDerivedKey, SpaceAccessError>;
}
