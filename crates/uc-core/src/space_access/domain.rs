use std::fmt;

use zeroize::Zeroize;

use crate::ids::{SessionId, SpaceId};

#[derive(Clone, Debug)]
pub struct SpaceAccessProofArtifact {
    pub pairing_session_id: SessionId,
    pub space_id: SpaceId,
    pub challenge_nonce: [u8; 32],
    pub proof_bytes: Vec<u8>,
}

/// Sponsor 发给 joiner 的 pairing offer——空间接入流程的载体。
///
/// `keyslot_blob` 是 adapter 自序列化的不透明字节（承载 KEK wrap 后的 MasterKey
/// 等加密物料），领域层不关心其布局；`challenge_nonce` 是 sponsor 产生的 32
/// 字节挑战值，joiner 用它 + 自身派生的 MasterKey 构造 proof 回传，sponsor
/// 验证 proof 以确认 joiner 拿到正确口令。
#[derive(Clone, Debug)]
pub struct JoinOffer {
    pub space_id: SpaceId,
    pub keyslot_blob: Vec<u8>,
    pub challenge_nonce: [u8; 32],
}

/// Pairing proof 链路上的不透明派生密钥。
///
/// 由 `SpaceAccessPort::derive_master_key_for_proof` 构造（adapter 内部
/// 从 keyslot 解出原始密钥字节后包装），传给 `ProofPort::build_proof`
/// 用于 HMAC 计算。两端都看不到 `MasterKey`——领域层只看到一段
/// "本次 proof 链路专用的 32 字节秘密"。
///
/// 不可 Clone / Serialize，drop 时自动清零。
pub struct ProofDerivedKey([u8; 32]);

impl ProofDerivedKey {
    /// adapter 内部按需构造——领域代码不应直接调用。
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// 借用底层字节用于 HMAC 计算等场景。
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for ProofDerivedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProofDerivedKey([REDACTED])")
    }
}

impl Drop for ProofDerivedKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
