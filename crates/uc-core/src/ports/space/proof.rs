use crate::ids::{SessionId, SpaceId};
use crate::space_access::{ProofDerivedKey, SpaceAccessProofArtifact};

#[async_trait::async_trait]
pub trait ProofPort: Send + Sync {
    /// 用 SpaceAccessPort 派生出的不透明凭据计算 HMAC proof。
    ///
    /// 签名里只出现领域级"本次 proof 链路的派生密钥"——adapter 内部如何
    /// 把它映射到具体算法（HMAC-SHA256 等）属于实现细节。
    async fn build_proof(
        &self,
        pairing_session_id: &SessionId,
        space_id: &SpaceId,
        challenge_nonce: [u8; 32],
        derived_key: &ProofDerivedKey,
    ) -> anyhow::Result<SpaceAccessProofArtifact>;

    async fn verify_proof(
        &self,
        proof: &SpaceAccessProofArtifact,
        expected_nonce: [u8; 32],
    ) -> anyhow::Result<bool>;
}
