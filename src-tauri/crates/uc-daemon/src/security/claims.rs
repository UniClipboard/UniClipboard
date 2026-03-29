//! JWT session token claims for daemon HTTP API authentication.
//!
//! Session tokens are HS256-signed JWTs containing client identity,
//! PID whitelist membership, and access level. They are short-lived
//! (5 minutes) and verified on every authenticated request.

use serde::{Deserialize, Serialize};

/// Issuer value embedded in all daemon session tokens.
pub const ISSUER: &str = "uniclipboard-daemon";

/// Subject value for all session tokens (tokens are issued to frontend/CLI clients).
pub const SUBJECT: &str = "frontend";

/// Token time-to-live in seconds (5 minutes).
pub const TTL_SECS: i64 = 300;

/// Access level 1: public endpoint (no auth required).
pub const LEVEL_L1: u8 = 1;

/// Access level 2: authenticated endpoint (valid JWT + PID required).
pub const LEVEL_L2: u8 = 2;

/// Token refresh threshold in seconds (4 minutes into the TTL window).
/// Clients should request a new token once 4 minutes have elapsed.
pub const REFRESH_AT_SECS: i64 = 240;

/// JWT session token claims for daemon HTTP API authentication.
///
/// Contains client identity (PID, client_type), access level, and encryption state.
/// Signed with HS256 using a 32-byte secret generated at daemon startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenClaims {
    /// Issuer — always "uniclipboard-daemon".
    pub iss: String,
    /// Subject — always "frontend".
    pub sub: String,
    /// Issued-at timestamp (Unix seconds).
    pub iat: i64,
    /// Expiration timestamp (Unix seconds, iat + 300).
    pub exp: i64,
    /// Client process ID used for PID whitelist verification.
    pub pid: u32,
    /// Client type: "gui", "cli", or "other".
    pub client_type: String,
    /// Unique token identifier (UUID v4 hex string).
    pub jti: String,
    /// Access level: 1 = L1 (public), 2 = L2 (authenticated).
    /// L3/L4 values are reserved for future phases.
    pub access_level: u8,
    /// Whether the client's encryption session is initialized.
    pub encryption_ready: bool,
}

impl SessionTokenClaims {
    /// Create a new session token with auto-generated JTI and timestamps.
    ///
    /// # Arguments
    /// * `pid` — Client process ID for PID whitelist verification.
    /// * `client_type` — Client kind: "gui", "cli", or "other".
    /// * `access_level` — Permission level (1 = L1, 2 = L2).
    /// * `encryption_ready` — Whether the client's encryption session is initialized.
    pub fn new(pid: u32, client_type: String, access_level: u8, encryption_ready: bool) -> Self {
        let now = chrono::Utc::now().timestamp();
        let jti = generate_jti();
        Self {
            iss: ISSUER.to_string(),
            sub: SUBJECT.to_string(),
            iat: now,
            exp: now + TTL_SECS,
            pid,
            client_type,
            jti,
            access_level,
            encryption_ready,
        }
    }

    /// Sign this claims set as an HS256 JWT using the provided secret.
    ///
    /// # Errors
    /// Returns a jsonwebtoken error if encoding fails.
    pub fn sign(&self, secret: &[u8; 32]) -> Result<String, jsonwebtoken::errors::Error> {
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        let header = Header::new(Algorithm::HS256);
        let key = EncodingKey::from_secret(secret);
        encode(&header, self, &key)
    }

    /// Verify and decode an HS256 JWT using the provided secret.
    ///
    /// Validates:
    /// - Signature matches using HS256 with the secret
    /// - Issuer is "uniclipboard-daemon"
    /// - Subject is "frontend"
    /// - Token has not expired
    ///
    /// # Errors
    /// Returns a jsonwebtoken error if validation fails.
    pub fn verify(token: &str, secret: &[u8; 32]) -> Result<Self, jsonwebtoken::errors::Error> {
        use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[ISSUER]);
        let key = DecodingKey::from_secret(secret);
        let token_data = decode::<SessionTokenClaims>(token, &key, &validation)?;
        let claims = token_data.claims;
        // Subject is validated manually since jsonwebtoken 10.x does not provide set_subject
        if claims.sub != SUBJECT {
            return Err(jsonwebtoken::errors::Error::from(
                jsonwebtoken::errors::ErrorKind::InvalidSubject,
            ));
        }
        Ok(claims)
    }
}

/// Generate a 32-character hex string (16 random bytes) for use as JTI.
fn generate_jti() -> String {
    let mut bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_sign_and_verify_roundtrip() {
        let secret = [0u8; 32];
        let claims = SessionTokenClaims::new(12345, "gui".into(), LEVEL_L2, false);
        let token = claims.sign(&secret).expect("sign should succeed");
        let verified = SessionTokenClaims::verify(&token, &secret).expect("verify should succeed");

        assert_eq!(verified.iss, ISSUER);
        assert_eq!(verified.sub, SUBJECT);
        assert_eq!(verified.pid, 12345);
        assert_eq!(verified.client_type, "gui");
        assert_eq!(verified.access_level, LEVEL_L2);
        assert!(!verified.encryption_ready);
        assert!(!verified.jti.is_empty());
    }

    #[test]
    fn claims_expired_token_rejected() {
        let secret = [0u8; 32];
        let mut claims = SessionTokenClaims::new(12345, "gui".into(), LEVEL_L2, false);
        // Backdate exp significantly to the past (7 days)
        claims.exp = chrono::Utc::now().timestamp() - 86400 * 7;
        let token = claims.sign(&secret).expect("sign should succeed");
        let result = SessionTokenClaims::verify(&token, &secret);
        assert!(result.is_err(), "expired token should be rejected");
    }

    #[test]
    fn claims_wrong_secret_rejected() {
        let secret_a = [1u8; 32];
        let secret_b = [2u8; 32];
        let claims = SessionTokenClaims::new(12345, "gui".into(), LEVEL_L2, false);
        let token = claims.sign(&secret_a).expect("sign should succeed");
        let result = SessionTokenClaims::verify(&token, &secret_b);
        assert!(result.is_err(), "token signed with wrong secret should be rejected");
    }

    #[test]
    fn claims_iss_validation() {
        let secret = [0u8; 32];
        let claims = SessionTokenClaims::new(12345, "gui".into(), LEVEL_L2, false);
        let token = claims.sign(&secret).expect("sign should succeed");

        // Decode with wrong issuer validation should fail
        use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&["wrong-issuer"]);
        let key = DecodingKey::from_secret(&secret);
        let result = decode::<SessionTokenClaims>(&token, &key, &validation);
        assert!(result.is_err(), "token with wrong issuer should be rejected");
    }
}
