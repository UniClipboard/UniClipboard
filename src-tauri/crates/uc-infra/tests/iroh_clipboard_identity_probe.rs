//! Slice 2 Phase 2 T2 — probe iroh 0.95 identity resolution for clipboard.
//!
//! Goal: confirm the receiver-side path `Connection::remote_id() → DeviceId`
//! can be reconstructed **without extending the port layer**, by deriving
//! the same `IdentityFingerprint` the pairing flow stored against the
//! remote's `SpaceMember`.
//!
//! The factory at `uc-infra/security::Sha256IdentityFingerprintFactory`
//! accepts an arbitrary `&[u8]` of length 32. Both
//!
//! * `SecretKey::public().as_bytes()` — used by `IrohIdentityStore` when
//!   persisting the fingerprint at A1/A2 + B2 pairing, and
//! * `EndpointId = iroh_base::PublicKey` returned from
//!   `Connection::remote_id()`
//!
//! project down to the same 32-byte Ed25519 compressed-edwards-y bytes.
//! So if both sides feed `public().as_bytes()` into the same factory, the
//! receiver's lookup-by-fingerprint query into `MemberRepositoryPort` will
//! hit.
//!
//! **Load-bearing invariant established here**: no new port, no
//! `IdentityFingerprintFactoryPort::from_ed25519_public_key` extension —
//! the existing `from_public_key(&[u8])` entry point is sufficient.
//!
//! Runs loopback-only (relays disabled) and uses a test-local ALPN so the
//! probe does not collide with production pairing / presence / clipboard
//! traffic.

use std::time::Duration;

use iroh::{Endpoint, RelayMode, SecretKey};
use uc_core::ports::security::IdentityFingerprintFactoryPort;
use uc_infra::security::Sha256IdentityFingerprintFactory;

const PROBE_ALPN: &[u8] = b"uniclipboard/clipboard-identity-probe/0";

/// Bind an endpoint with a deterministic 32-byte secret so the
/// fingerprint assertion has a fixed expected value.
async fn bind_with_secret(seed: [u8; 32]) -> Endpoint {
    let sk = SecretKey::from_bytes(&seed);
    Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(sk)
        .alpns(vec![PROBE_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("bind endpoint")
}

async fn wait_for_direct_addrs(endpoint: &Endpoint) {
    for _ in 0..100 {
        if !endpoint.addr().addrs.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("endpoint never published direct addresses");
}

/// Verdict 1 — the main question. Sponsor accepts a connection from joiner,
/// takes `connection.remote_id().as_bytes()`, runs it through the same
/// `Sha256IdentityFingerprintFactory` that `IrohIdentityStore` used when
/// the joiner's fingerprint was originally persisted, and recovers the
/// identical `IdentityFingerprint`.
#[tokio::test]
async fn remote_id_public_key_feeds_same_factory_as_identity_store() {
    let joiner_seed = [0x11u8; 32];
    let sponsor_seed = [0x22u8; 32];

    // Expected fingerprint: what `IrohIdentityStore::derive_fingerprint`
    // computes for the joiner at B2. This is the value the pairing flow
    // persists into `SpaceMember.identity_fingerprint` on the sponsor side.
    let factory = Sha256IdentityFingerprintFactory;
    let joiner_sk = SecretKey::from_bytes(&joiner_seed);
    let expected_fp = factory
        .from_public_key(joiner_sk.public().as_bytes())
        .expect("derive expected fingerprint from joiner secret");

    let joiner = bind_with_secret(joiner_seed).await;
    let sponsor = bind_with_secret(sponsor_seed).await;
    wait_for_direct_addrs(&joiner).await;
    wait_for_direct_addrs(&sponsor).await;

    let sponsor_addr = sponsor.addr();

    // Sponsor extracts remote_id bytes from the first inbound connection,
    // derives a fingerprint via the shared factory, and hands it back.
    let (fp_tx, fp_rx) = tokio::sync::oneshot::channel::<uc_core::security::IdentityFingerprint>();
    let sponsor_task = tokio::spawn(async move {
        let factory = Sha256IdentityFingerprintFactory;
        let incoming = sponsor
            .accept()
            .await
            .expect("accept yields an inbound connection");
        let conn = incoming.await.expect("connection opens");
        let remote = conn.remote_id();
        let derived = factory
            .from_public_key(remote.as_bytes())
            .expect("factory derives fingerprint from remote_id bytes");
        fp_tx.send(derived).expect("oneshot receiver alive");
        // Hold the connection open so the dial side can observe a clean close.
        let _ = conn.closed().await;
    });

    let _conn = joiner
        .connect(sponsor_addr, PROBE_ALPN)
        .await
        .expect("joiner connects to sponsor");

    let recovered = tokio::time::timeout(Duration::from_secs(3), fp_rx)
        .await
        .expect("oneshot resolves in time")
        .expect("sender not dropped");

    assert_eq!(
        recovered, expected_fp,
        "fingerprint derived from Connection::remote_id().as_bytes() must \
         match fingerprint derived from SecretKey::public().as_bytes() — \
         both are the same 32-byte Ed25519 compressed-edwards-y and the \
         factory is domain-separated deterministic SHA-256"
    );

    drop(sponsor_task);
}

/// Verdict 2 — factory is deterministic across independent invocations on
/// the same bytes. The clipboard receiver adapter will derive the
/// fingerprint on every inbound connection; drift here would manifest as
/// "unknown peer" rejections under production load.
#[test]
fn factory_is_deterministic_across_repeated_calls() {
    let factory = Sha256IdentityFingerprintFactory;
    let bytes = [0xAAu8; 32];
    let first = factory
        .from_public_key(&bytes)
        .expect("first derivation succeeds");
    let second = factory
        .from_public_key(&bytes)
        .expect("second derivation succeeds");
    assert_eq!(first, second);
}

/// Verdict 3 — different secrets yield different fingerprints, so the
/// lookup-by-fingerprint strategy cannot false-positive across members.
#[test]
fn different_pubkeys_yield_different_fingerprints() {
    let factory = Sha256IdentityFingerprintFactory;
    let fp_a = factory.from_public_key(&[0x01u8; 32]).expect("derive a");
    let fp_b = factory.from_public_key(&[0x02u8; 32]).expect("derive b");
    assert_ne!(fp_a, fp_b);
}
