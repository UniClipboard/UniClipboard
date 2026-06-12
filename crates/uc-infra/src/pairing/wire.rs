//! Binary codec for [`PairingSessionMessage`].
//!
//! Slice 1 pairing session wire format (postcard + explicit version byte).
//! Runs over an iroh bi-directional stream; P7c.2 layers length-prefixed
//! framing on top of this codec before hitting the stream.
//!
//! Design notes:
//!
//! * **Wire types are infra-local.** The core [`PairingSessionMessage`]
//!   carries no `serde` derives (§6.3). This module owns mirror structs with
//!   serde derives and maps them at the boundary.
//! * **Envelope carries a version byte from day 1.** Slice 2+ will extend
//!   the enum (e.g. keep-alives, resume tokens); `v` lets us distinguish
//!   "old peer sent unknown variant" from "data corruption".
//! * **postcard, not JSON.** postcard gives ~40% smaller payloads than
//!   JSON for this shape (mainly because keyslot / challenge / nonce are
//!   binary bytes). Rendezvous tickets are already ~500 bytes — saving here
//!   is worth the binary opaqueness.
//! * **IdentityFingerprint on the wire uses the display form**
//!   (`ABCD-EFGH-IJKL-MNOP`) — stable, printable in logs, round-trips
//!   through [`IdentityFingerprint::from_display_string`].
//!
//! [`PairingSessionMessage`]: uc_core::pairing::PairingSessionMessage

use serde::{Deserialize, Serialize};
use thiserror::Error;

use uc_core::ids::{DeviceId, SpaceId};
use uc_core::pairing::{
    InvitationCode, JoinerChallengeResponse, JoinerRequest, PairingReject, PairingRejectReason,
    PairingSessionMessage, SponsorConfirm, SponsorKeyslotOffer,
};
use uc_core::ports::pairing::PairingSessionId;
use uc_core::security::IdentityFingerprint;

/// Wire 版本号。
///
/// 升版历史：
/// - v1 → v2（Slice 2 Phase 1 · T5）：在 `JoinerRequest` 与 `SponsorConfirm`
///   上新增 `transport_address_blob` 字段。
/// - v2 → v3（Phase 098）：在 `SponsorConfirm` 上新增 `sponsor_space_person_id:
///   Option<String>` 字段，把 sponsor 的 telemetry person 标识派给 joiner。
///   字段是 `Option`：sponsor 端 telemetry 身份未确立时编为 `None`，joiner 端
///   见 `None` 退回 Solo（schema doc §3.4 / task_plan §开放问题 2 决策 A）。
///
/// postcard 非 schema-兼容，每次新增字段都升版本号；旧 peer 发来的低版本帧会走
/// [`WireDecodeError::UnsupportedVersion`] 分支显式拒连，让排障信号明确。
const WIRE_VERSION: u8 = 3;

// ============================================================================
// Wire types (infra-local)
// ============================================================================

#[derive(Serialize, Deserialize, Debug)]
struct WireEnvelope {
    v: u8,
    body: WireBody,
}

#[derive(Serialize, Deserialize, Debug)]
enum WireBody {
    Request(WireJoinerRequest),
    KeyslotOffer(WireSponsorKeyslotOffer),
    ChallengeResponse(WireJoinerChallengeResponse),
    Confirm(WireSponsorConfirm),
    Reject(WirePairingReject),
}

#[derive(Serialize, Deserialize, Debug)]
struct WireJoinerRequest {
    invitation_code: String,
    device_id: String,
    device_name: String,
    identity_fingerprint: String,
    nonce: Vec<u8>,
    /// Slice 2 Phase 1 · T5：joiner 传输地址不透明字节（iroh postcard
    /// 编码的 `EndpointAddr`）。
    ///
    /// postcard 按结构体字段顺序追加，新增字段不是 schema-兼容的——
    /// Slice 1→Slice 2 升级期的跨版本对端不兼容通过 [`WIRE_VERSION`]
    /// 升到 `2` 来显式拒连，由 [`WireDecodeError::UnsupportedVersion`]
    /// 提示用户升级；生产前未发布，不需要兼容层。
    ///
    /// 空 `Vec` 是一个合法业务值：表示本端 adapter 暂时没有可发布的
    /// direct addr（例如 endpoint 还未 online），sponsor 端收到后跳过
    /// `peer_addr_repo.upsert`，presence 下次 `ensure_reachable_all`
    /// 从 rendezvous 再拉兜底。
    transport_address_blob: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
struct WireSponsorKeyslotOffer {
    space_id: String,
    keyslot_blob: Vec<u8>,
    challenge: Vec<u8>,
    pairing_session_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct WireJoinerChallengeResponse {
    encrypted_challenge: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
struct WireSponsorConfirm {
    space_id: String,
    sender_device_id: String,
    sender_device_name: String,
    sender_identity_fingerprint: String,
    /// Slice 2 Phase 1 · T5：sponsor 传输地址不透明字节。详见
    /// [`WireJoinerRequest::transport_address_blob`] 的说明。
    transport_address_blob: Vec<u8>,
    /// Phase 098：sponsor 派发给 joiner 的 telemetry person 标识。
    ///
    /// 字段含义见 [`SponsorConfirm::sponsor_space_person_id`]。
    /// 编为 `Option<String>`（postcard `Option` 占 1 byte tag）以支持 sponsor
    /// 端尚未持久化 `space_person_id` 时（v1→v2 老用户升级未配对）的 `None`
    /// 形态；joiner 端收到 `None` 退回 Solo。
    ///
    /// 用 String 而非 uuid 类型保持与 `space_id` / `sender_device_id` 字段统一
    /// 的字符串形态，wire 层不引入额外类型依赖；core 端 `SponsorConfirm` 把它
    /// 解析为 `Uuid`。
    sponsor_space_person_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct WirePairingReject {
    reason: WireRejectReason,
}

#[derive(Serialize, Deserialize, Debug)]
enum WireRejectReason {
    InvitationMismatch,
    PassphraseMismatch,
    UserRejected,
    Timeout,
    Internal(String),
}

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum WireEncodeError {
    #[error("postcard encode failed: {0}")]
    Postcard(#[from] postcard::Error),
}

#[derive(Debug, Error)]
pub enum WireDecodeError {
    #[error("postcard decode failed: {0}")]
    Postcard(postcard::Error),

    #[error("unsupported wire version {got} (this build understands {expected})")]
    UnsupportedVersion { got: u8, expected: u8 },

    #[error("invalid identity fingerprint on wire: {0}")]
    InvalidFingerprint(String),

    /// Phase 098：sponsor 派发的 `space_person_id` 字符串无法解析为 UUID。
    /// 不算致命错误（telemetry 字段而已）但仍会让整条 confirm 拒收，
    /// 触发对端重连——避免在 wire 上接受半破损的字段。
    #[error("invalid sponsor_space_person_id on wire: {0}")]
    InvalidSpacePersonId(String),
}

impl From<postcard::Error> for WireDecodeError {
    fn from(err: postcard::Error) -> Self {
        WireDecodeError::Postcard(err)
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Serialize a [`PairingSessionMessage`] for transport.
pub fn encode(message: &PairingSessionMessage) -> Result<Vec<u8>, WireEncodeError> {
    let envelope = WireEnvelope {
        v: WIRE_VERSION,
        body: to_wire(message),
    };
    Ok(postcard::to_allocvec(&envelope)?)
}

/// Deserialize a [`PairingSessionMessage`] from bytes produced by
/// [`encode`] (or a peer running a compatible version).
pub fn decode(bytes: &[u8]) -> Result<PairingSessionMessage, WireDecodeError> {
    let envelope: WireEnvelope = postcard::from_bytes(bytes)?;
    if envelope.v != WIRE_VERSION {
        return Err(WireDecodeError::UnsupportedVersion {
            got: envelope.v,
            expected: WIRE_VERSION,
        });
    }
    from_wire(envelope.body)
}

// ============================================================================
// Conversions
// ============================================================================

fn to_wire(msg: &PairingSessionMessage) -> WireBody {
    match msg {
        PairingSessionMessage::Request(r) => WireBody::Request(WireJoinerRequest {
            invitation_code: r.invitation_code.as_str().to_string(),
            device_id: r.device_id.as_str().to_string(),
            device_name: r.device_name.clone(),
            identity_fingerprint: r.identity_fingerprint.as_display().to_string(),
            nonce: r.nonce.clone(),
            transport_address_blob: r.transport_address_blob.clone(),
        }),
        PairingSessionMessage::KeyslotOffer(o) => WireBody::KeyslotOffer(WireSponsorKeyslotOffer {
            space_id: o.space_id.inner().clone(),
            keyslot_blob: o.keyslot_blob.clone(),
            challenge: o.challenge.clone(),
            pairing_session_id: o.pairing_session_id.as_str().to_string(),
        }),
        PairingSessionMessage::ChallengeResponse(c) => {
            WireBody::ChallengeResponse(WireJoinerChallengeResponse {
                encrypted_challenge: c.encrypted_challenge.clone(),
            })
        }
        PairingSessionMessage::Confirm(c) => WireBody::Confirm(WireSponsorConfirm {
            space_id: c.space_id.inner().clone(),
            sender_device_id: c.sender_device_id.as_str().to_string(),
            sender_device_name: c.sender_device_name.clone(),
            sender_identity_fingerprint: c.sender_identity_fingerprint.as_display().to_string(),
            transport_address_blob: c.transport_address_blob.clone(),
            sponsor_space_person_id: c.sponsor_space_person_id.map(|id| id.to_string()),
        }),
        PairingSessionMessage::Reject(r) => WireBody::Reject(WirePairingReject {
            reason: match &r.reason {
                PairingRejectReason::InvitationMismatch => WireRejectReason::InvitationMismatch,
                PairingRejectReason::PassphraseMismatch => WireRejectReason::PassphraseMismatch,
                PairingRejectReason::UserRejected => WireRejectReason::UserRejected,
                PairingRejectReason::Timeout => WireRejectReason::Timeout,
                PairingRejectReason::Internal(s) => WireRejectReason::Internal(s.clone()),
            },
        }),
    }
}

fn from_wire(body: WireBody) -> Result<PairingSessionMessage, WireDecodeError> {
    match body {
        WireBody::Request(r) => Ok(PairingSessionMessage::Request(JoinerRequest {
            invitation_code: InvitationCode::new(r.invitation_code),
            device_id: DeviceId::new(r.device_id),
            device_name: r.device_name,
            identity_fingerprint: parse_fingerprint(&r.identity_fingerprint)?,
            nonce: r.nonce,
            transport_address_blob: r.transport_address_blob,
        })),
        WireBody::KeyslotOffer(o) => Ok(PairingSessionMessage::KeyslotOffer(SponsorKeyslotOffer {
            space_id: SpaceId::from_string(o.space_id),
            keyslot_blob: o.keyslot_blob,
            challenge: o.challenge,
            pairing_session_id: PairingSessionId::new(o.pairing_session_id),
        })),
        WireBody::ChallengeResponse(c) => Ok(PairingSessionMessage::ChallengeResponse(
            JoinerChallengeResponse {
                encrypted_challenge: c.encrypted_challenge,
            },
        )),
        WireBody::Confirm(c) => Ok(PairingSessionMessage::Confirm(SponsorConfirm {
            space_id: SpaceId::from_string(c.space_id),
            sender_device_id: DeviceId::new(c.sender_device_id),
            sender_device_name: c.sender_device_name,
            sender_identity_fingerprint: parse_fingerprint(&c.sender_identity_fingerprint)?,
            transport_address_blob: c.transport_address_blob,
            sponsor_space_person_id: c
                .sponsor_space_person_id
                .map(|s| {
                    uuid::Uuid::parse_str(&s).map_err(|e| {
                        WireDecodeError::InvalidSpacePersonId(format!("{e} (got `{s}`)"))
                    })
                })
                .transpose()?,
        })),
        WireBody::Reject(r) => Ok(PairingSessionMessage::Reject(PairingReject {
            reason: match r.reason {
                WireRejectReason::InvitationMismatch => PairingRejectReason::InvitationMismatch,
                WireRejectReason::PassphraseMismatch => PairingRejectReason::PassphraseMismatch,
                WireRejectReason::UserRejected => PairingRejectReason::UserRejected,
                WireRejectReason::Timeout => PairingRejectReason::Timeout,
                WireRejectReason::Internal(s) => PairingRejectReason::Internal(s),
            },
        })),
    }
}

fn parse_fingerprint(s: &str) -> Result<IdentityFingerprint, WireDecodeError> {
    IdentityFingerprint::from_display_string(s)
        .map_err(|e| WireDecodeError::InvalidFingerprint(e.to_string()))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fingerprint() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap()
    }

    fn round_trip(msg: PairingSessionMessage) -> PairingSessionMessage {
        let bytes = encode(&msg).expect("encode");
        decode(&bytes).expect("decode")
    }

    #[test]
    fn request_round_trips() {
        let original = PairingSessionMessage::Request(JoinerRequest {
            invitation_code: InvitationCode::new("CODE-1234"),
            device_id: DeviceId::new("dev-001"),
            device_name: "Alice's laptop".to_string(),
            identity_fingerprint: sample_fingerprint(),
            nonce: vec![1, 2, 3, 4, 5],
            transport_address_blob: vec![0x9a, 0x01, 0x02],
        });

        let decoded = round_trip(original);
        match decoded {
            PairingSessionMessage::Request(r) => {
                assert_eq!(r.invitation_code.as_str(), "CODE-1234");
                assert_eq!(r.device_id.as_str(), "dev-001");
                assert_eq!(r.device_name, "Alice's laptop");
                assert_eq!(r.identity_fingerprint, sample_fingerprint());
                assert_eq!(r.nonce, vec![1, 2, 3, 4, 5]);
                assert_eq!(r.transport_address_blob, vec![0x9a, 0x01, 0x02]);
            }
            other => panic!("expected Request, got {other:?}"),
        }
    }

    #[test]
    fn keyslot_offer_round_trips() {
        let original = PairingSessionMessage::KeyslotOffer(SponsorKeyslotOffer {
            space_id: SpaceId::from_str("space-42"),
            keyslot_blob: vec![0xde, 0xad, 0xbe, 0xef],
            challenge: vec![0x01; 32],
            pairing_session_id: PairingSessionId::new("sess-abc-42"),
        });

        let decoded = round_trip(original);
        match decoded {
            PairingSessionMessage::KeyslotOffer(o) => {
                assert_eq!(o.space_id.inner(), "space-42");
                assert_eq!(o.keyslot_blob, vec![0xde, 0xad, 0xbe, 0xef]);
                assert_eq!(o.challenge, vec![0x01; 32]);
                assert_eq!(o.pairing_session_id.as_str(), "sess-abc-42");
            }
            other => panic!("expected KeyslotOffer, got {other:?}"),
        }
    }

    #[test]
    fn challenge_response_round_trips() {
        let original = PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
            encrypted_challenge: vec![0x42; 48],
        });
        let decoded = round_trip(original);
        match decoded {
            PairingSessionMessage::ChallengeResponse(c) => {
                assert_eq!(c.encrypted_challenge, vec![0x42; 48]);
            }
            other => panic!("expected ChallengeResponse, got {other:?}"),
        }
    }

    #[test]
    fn confirm_round_trips() {
        let space_person = uuid::Uuid::parse_str("018f0000-0000-7000-8000-00000000000a").unwrap();
        let original = PairingSessionMessage::Confirm(SponsorConfirm {
            space_id: SpaceId::from_str("space-99"),
            sender_device_id: DeviceId::new("dev-sponsor"),
            sender_device_name: "Bob's desktop".to_string(),
            sender_identity_fingerprint: sample_fingerprint(),
            transport_address_blob: vec![0xaa, 0xbb, 0xcc],
            sponsor_space_person_id: Some(space_person),
        });
        let decoded = round_trip(original);
        match decoded {
            PairingSessionMessage::Confirm(c) => {
                assert_eq!(c.space_id.inner(), "space-99");
                assert_eq!(c.sender_device_id.as_str(), "dev-sponsor");
                assert_eq!(c.sender_device_name, "Bob's desktop");
                assert_eq!(c.sender_identity_fingerprint, sample_fingerprint());
                assert_eq!(c.transport_address_blob, vec![0xaa, 0xbb, 0xcc]);
                assert_eq!(c.sponsor_space_person_id, Some(space_person));
            }
            other => panic!("expected Confirm, got {other:?}"),
        }
    }

    /// PR 5：sponsor 端尚未持久化 `space_person_id`（v1→v2 老用户升级未配对）
    /// 时，Confirm 上的字段应为 `None`，wire round-trip 后保持 `None`，
    /// joiner 端按 Solo 退化（task_plan §开放问题 2 决策 A）。
    #[test]
    fn confirm_round_trips_with_none_sponsor_space_person_id() {
        let original = PairingSessionMessage::Confirm(SponsorConfirm {
            space_id: SpaceId::from_str("space-99"),
            sender_device_id: DeviceId::new("dev-sponsor"),
            sender_device_name: "Bob's desktop".to_string(),
            sender_identity_fingerprint: sample_fingerprint(),
            transport_address_blob: vec![],
            sponsor_space_person_id: None,
        });
        let decoded = round_trip(original);
        match decoded {
            PairingSessionMessage::Confirm(c) => {
                assert_eq!(
                    c.sponsor_space_person_id, None,
                    "None 必须 round-trip 为 None，让 joiner 退回 Solo"
                );
            }
            other => panic!("expected Confirm, got {other:?}"),
        }
    }

    #[test]
    fn reject_round_trips_all_reasons() {
        for reason in [
            PairingRejectReason::InvitationMismatch,
            PairingRejectReason::PassphraseMismatch,
            PairingRejectReason::UserRejected,
            PairingRejectReason::Timeout,
            PairingRejectReason::Internal("bad things".to_string()),
        ] {
            let original = PairingSessionMessage::Reject(PairingReject {
                reason: reason.clone(),
            });
            let decoded = round_trip(original);
            match decoded {
                PairingSessionMessage::Reject(r) => assert_eq!(r.reason, reason),
                other => panic!("expected Reject, got {other:?}"),
            }
        }
    }

    #[test]
    fn decode_rejects_future_version() {
        // Build a forged envelope at v = WIRE_VERSION + 1 to verify
        // rejection semantics survive future bumps without touching this
        // test's hardcoded numbers.
        #[derive(Serialize)]
        struct FutureEnvelope {
            v: u8,
            body: WireBody,
        }
        let fake_version = WIRE_VERSION + 1;
        let fake = FutureEnvelope {
            v: fake_version,
            body: WireBody::ChallengeResponse(WireJoinerChallengeResponse {
                encrypted_challenge: vec![],
            }),
        };
        let bytes = postcard::to_allocvec(&fake).unwrap();

        match decode(&bytes) {
            Err(WireDecodeError::UnsupportedVersion { got, expected }) => {
                assert_eq!(got, fake_version);
                assert_eq!(expected, WIRE_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_garbage_bytes() {
        let garbage = vec![0xff; 16];
        match decode(&garbage) {
            Err(WireDecodeError::Postcard(_)) => {}
            other => panic!("expected Postcard error, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_invalid_fingerprint_format() {
        // Manually build a request with a too-short fingerprint on the wire.
        let fake = WireEnvelope {
            v: WIRE_VERSION,
            body: WireBody::Request(WireJoinerRequest {
                invitation_code: "x".to_string(),
                device_id: "d".to_string(),
                device_name: "n".to_string(),
                identity_fingerprint: "TOO_SHORT".to_string(),
                nonce: vec![],
                transport_address_blob: vec![],
            }),
        };
        let bytes = postcard::to_allocvec(&fake).unwrap();

        match decode(&bytes) {
            Err(WireDecodeError::InvalidFingerprint(msg)) => {
                assert!(
                    msg.contains("expected 16 characters"),
                    "unexpected error body: {msg}"
                );
            }
            other => panic!("expected InvalidFingerprint, got {other:?}"),
        }
    }

    #[test]
    fn encoded_payload_is_binary_and_nontrivial() {
        let msg = PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
            encrypted_challenge: vec![1, 2, 3],
        });
        let bytes = encode(&msg).unwrap();
        assert!(!bytes.is_empty());
        // Envelope version byte should be the first byte for postcard's
        // layout of `struct { v: u8, body: enum }`.
        assert_eq!(bytes[0], WIRE_VERSION);
    }
}
