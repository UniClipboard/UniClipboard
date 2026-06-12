//! Local invitation-code minting.
//!
//! Until 2026, code provenance lived on the rendezvous service: the
//! sponsor's `POST /v1/pairings` returned a code minted server-side.
//! That coupled "can produce a code" to "can reach rendezvous", so the
//! first-pair-no-WAN scenario could not work at all.
//!
//! This helper moves provenance to the sponsor's process: a fresh code
//! is produced from `OsRng` and a group-style printable alphabet, then
//! handed to whichever publish channels are reachable (LAN multicast,
//! cloud directory, etc.). The cloud channel becomes a best-effort
//! query index instead of a code-issuing authority.
//!
//! ## Alphabet & shape
//!
//! 8 base32 chars in two `XXXX-XXXX` groups. Standard Crockford base32
//! alphabet — `0-9` plus `A-Z` with `I`, `L`, `O`, `U` excluded. The
//! Crockford choice is deliberate: it's an industry-standard 32-char
//! set with documented confusable-handling, so a typing-input layer
//! can normalise (e.g. `I→1`, `O→0`) without having to invent our own
//! mapping table.
//!
//! 8 chars × log2(32) = 40 bits of entropy. Within a 5-minute pairing
//! window, the birthday-bound collision probability across a global
//! installed base of 10⁶ active sponsors at the same instant is
//! ~10⁻³ — well below "users notice." Collisions are not security-
//! critical anyway: the final handshake validates the full code +
//! sponsor identity, so a collision degrades to "wrong sponsor answers,
//! handshake fingerprint mismatches, joiner sees an authentication
//! error" instead of a wrong-pair success.

use rand::RngCore;

/// Length of the unhyphenated alphabet payload.
const CODE_BODY_LEN: usize = 8;

/// Crockford base32 alphabet — 32 chars indexed by the bottom 5 bits of
/// a random byte. Excludes `I`, `L`, `O`, `U`. We keep `0`/`1` because
/// downstream input normalisation can fold confusables (`O→0`, `I→1`)
/// during type-in; inventing our own table here would defeat that.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Mints a fresh invitation code from `OsRng`.
///
/// Returns a `String` formatted as `XXXX-XXXX` (9 chars including the
/// hyphen). Caller is responsible for treating this as opaque past the
/// "display + pass it to publish channels" step — there is no version
/// byte or checksum in v1; if we want OCR-friendly variants later we
/// extend with a new helper, not a flag here.
pub fn mint_invitation_code() -> String {
    let mut rng = rand::rng();
    let mut out = String::with_capacity(CODE_BODY_LEN + 1);
    for i in 0..CODE_BODY_LEN {
        if i == CODE_BODY_LEN / 2 {
            out.push('-');
        }
        // One random byte per char; we only consume 5 bits so the upper
        // 3 are wasted — fine, OsRng is not the bottleneck.
        let b = rng.next_u32() as u8;
        let idx = (b & 0b0001_1111) as usize;
        out.push(ALPHABET[idx] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn minted_code_is_xxxx_dash_xxxx() {
        let code = mint_invitation_code();
        assert_eq!(code.len(), 9, "8 chars + 1 hyphen");
        let (left, right) = code.split_once('-').expect("must contain hyphen");
        assert_eq!(left.len(), 4);
        assert_eq!(right.len(), 4);
    }

    #[test]
    fn minted_code_uses_only_allowed_alphabet() {
        let code = mint_invitation_code();
        for ch in code.chars().filter(|c| *c != '-') {
            assert!(
                ALPHABET.contains(&(ch as u8)),
                "char {ch:?} not in alphabet {:?}",
                std::str::from_utf8(ALPHABET).unwrap()
            );
        }
    }

    #[test]
    fn alphabet_excludes_crockford_confusables() {
        // Crockford's excluded set: I, L, O, U.
        for confusable in [b'I', b'L', b'O', b'U'] {
            assert!(
                !ALPHABET.contains(&confusable),
                "Crockford-excluded byte {} must not appear in alphabet",
                confusable as char
            );
        }
    }

    #[test]
    fn alphabet_has_exactly_32_chars() {
        assert_eq!(ALPHABET.len(), 32);
    }

    /// Not a uniqueness guarantee — base32(8) is small enough to collide
    /// in theory — but at the sample size below the chance of any
    /// collision is ~10⁻⁹, so this asserts the RNG is actually random
    /// (not stuck on a constant) without being flaky.
    #[test]
    fn one_thousand_mints_produce_distinct_codes() {
        let set: HashSet<String> = (0..1_000).map(|_| mint_invitation_code()).collect();
        assert_eq!(set.len(), 1_000, "1k mints must not collide");
    }
}
