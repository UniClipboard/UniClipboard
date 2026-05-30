//! `uniclip join` — joiner side of Slice 1 pairing.
//!
//! Takes an invitation code and passphrase, then drives
//! [`SpaceSetupFacade::redeem_pairing_invitation`] to completion.
//! Unlike [`invite`](super::invite), this command is a single blocking
//! RPC — B2 owns its own dial/wait loop internally, so we simply await
//! the result (with Ctrl+C handling to guarantee clean iroh teardown).

use tokio::select;
use tokio::signal;

use uc_application::facade::space_setup::{
    RedeemPairingInvitationError, RedeemPairingInvitationInput,
};

use crate::commands::app_session::{
    build_app_session, default_device_name, refuse_if_daemon_running,
};
use crate::exit_codes;
use crate::ui;

const EXIT_SIGINT: i32 = 130;

/// Number of base32 chars in an invitation-code body (the `XXXX-XXXX`
/// shape carries 8 chars plus one middle hyphen).
const CODE_BODY_LEN: usize = 8;

/// Fold a typed invitation code into the canonical `XXXX-XXXX` form the
/// sponsor minted and published.
///
/// Codes use an all-uppercase Crockford base32 alphabet and are compared
/// byte-for-byte (rendezvous lookup key + handshake), so loose typing
/// would otherwise fail to pair. We drop separators (whitespace, hyphens),
/// uppercase, and — when exactly the 8-char body remains — re-insert the
/// single middle hyphen. Anything else is passed through compacted and
/// uppercased so a genuinely malformed code still surfaces a real
/// resolution error instead of being silently "fixed".
fn normalize_invitation_code(raw: &str) -> String {
    let compact: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_ascii_uppercase();
    if compact.is_ascii() && compact.len() == CODE_BODY_LEN {
        let mid = CODE_BODY_LEN / 2;
        format!("{}-{}", &compact[..mid], &compact[mid..])
    } else {
        compact
    }
}

pub struct JoinArgs {
    pub code: Option<String>,
    pub passphrase: Option<String>,
    pub device_name: Option<String>,
}

pub async fn run(args: JoinArgs, verbose: bool) -> i32 {
    ui::header("Join a space");

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let bundle = match build_app_session(verbose).await {
        Ok(b) => b,
        Err(code) => return code,
    };

    // Invitation codes are minted from an all-uppercase Crockford base32
    // alphabet and matched by an exact string compare (rendezvous lookup
    // key + handshake). Fold typed input to the canonical `XXXX-XXXX`
    // form here so a lowercased or hyphen-less code still pairs.
    let code_str = match args.code {
        Some(c) if !c.trim().is_empty() => normalize_invitation_code(&c),
        Some(_) => {
            ui::error("--code is empty");
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password("Invitation code") {
            Ok(c) if !c.trim().is_empty() => normalize_invitation_code(&c),
            Ok(_) => {
                ui::error("Invitation code cannot be empty");
                bundle.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
            Err(e) => {
                ui::error(&e);
                bundle.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    let passphrase_str = match args.passphrase {
        Some(p) if !p.trim().is_empty() => p,
        Some(_) => {
            ui::error("--passphrase is empty");
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password("Space passphrase") {
            Ok(p) if !p.trim().is_empty() => p,
            Ok(_) => {
                ui::error("Passphrase cannot be empty");
                bundle.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
            Err(e) => {
                ui::error(&e);
                bundle.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    // B2's use case reads `Settings.general.device_name` from disk rather
    // than taking it in the command, so if this is a brand-new profile
    // the setting will be absent and `redeem` fails with
    // `DeviceNameRequired`. Mirror the init command's behaviour:
    // `--device-name` overrides, otherwise default to the OS hostname.
    // Persist to settings before dialing.
    let device_name = args
        .device_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(default_device_name);
    let device_name = match device_name {
        Some(n) => n,
        None => {
            ui::error("device name is required (pass --device-name or set a system hostname)");
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };
    if let Err(err) = bundle
        .app_facade()
        .set_device_name(device_name.clone())
        .await
    {
        ui::error(&format!("failed to persist device_name: {err}"));
        bundle.shutdown().await;
        return exit_codes::EXIT_ERROR;
    }

    let input = RedeemPairingInvitationInput {
        code: code_str,
        passphrase: passphrase_str,
    };

    let spinner = ui::spinner("Dialing sponsor and running handshake...");

    // Clone the Arc so the in-flight future does not borrow `bundle`
    // — otherwise `bundle.shutdown().await` below can't take
    // ownership.
    let facade = std::sync::Arc::clone(bundle.app_facade());
    let redeem = async move { facade.redeem_pairing_invitation(input).await };
    tokio::pin!(redeem);

    let exit = select! {
        result = &mut redeem => match result {
            Ok(out) => {
                ui::spinner_finish_success(&spinner, "Joined space");
                ui::info("space_id", out.space_id.as_str());
                ui::info("self_device_id", out.self_device_id.as_str());
                ui::info("self_device_name", &device_name);
                ui::info("self_fingerprint", &out.self_identity_fingerprint.to_string());
                ui::info("sponsor_device_id", out.sponsor_device_id.as_str());
                ui::info("sponsor_fingerprint", &out.sponsor_identity_fingerprint.to_string());
                exit_codes::EXIT_SUCCESS
            }
            Err(err) => {
                let hint = match &err {
                    RedeemPairingInvitationError::InvitationNotFound => {
                        "Double-check the code — sponsor may have let it expire or reissued."
                    }
                    RedeemPairingInvitationError::InvitationExpired => {
                        "Ask the sponsor to run `invite` again to issue a fresh code."
                    }
                    RedeemPairingInvitationError::PassphraseMismatch => {
                        "Passphrase did not match the sponsor's. Retry `join`."
                    }
                    RedeemPairingInvitationError::SponsorUnreachable => {
                        "Sponsor is online in rendezvous but could not be reached. Check NAT / relay."
                    }
                    RedeemPairingInvitationError::ServiceUnavailable => {
                        "Rendezvous service is unreachable."
                    }
                    _ => "",
                };
                ui::spinner_finish_error(&spinner, &format!("Join failed: {err}"));
                if !hint.is_empty() {
                    ui::info("hint", hint);
                }
                exit_codes::EXIT_ERROR
            }
        },
        _ = signal::ctrl_c() => {
            ui::spinner_finish_error(&spinner, "Interrupted by user");
            EXIT_SIGINT
        }
    };

    bundle.shutdown().await;
    exit
}

#[cfg(test)]
mod tests {
    use super::normalize_invitation_code;

    #[test]
    fn already_canonical_code_is_unchanged() {
        assert_eq!(normalize_invitation_code("ABCD-1234"), "ABCD-1234");
    }

    #[test]
    fn lowercase_is_uppercased() {
        assert_eq!(normalize_invitation_code("abcd-1234"), "ABCD-1234");
    }

    #[test]
    fn hyphenless_eight_chars_get_canonical_hyphen() {
        assert_eq!(normalize_invitation_code("abcd1234"), "ABCD-1234");
        assert_eq!(normalize_invitation_code("ABCD1234"), "ABCD-1234");
    }

    #[test]
    fn surrounding_and_inner_whitespace_is_dropped() {
        assert_eq!(normalize_invitation_code("  abcd 1234 "), "ABCD-1234");
        assert_eq!(normalize_invitation_code("ABCD - 1234"), "ABCD-1234");
    }

    #[test]
    fn malformed_length_is_passed_through_compacted() {
        // Not 8 body chars → no hyphen reconstruction, but still
        // separator-stripped + uppercased so resolution fails on the
        // real value rather than a half-normalised one.
        assert_eq!(normalize_invitation_code("abc123"), "ABC123");
        assert_eq!(normalize_invitation_code("abcde-12345"), "ABCDE12345");
    }

    #[test]
    fn non_ascii_input_is_passed_through_without_slicing() {
        // Non-ASCII means the `is_ascii()` guard skips hyphen
        // reconstruction (and byte-slicing), so we never panic on a char
        // boundary. ASCII letters still uppercase; `é` is left as-is.
        assert_eq!(normalize_invitation_code("abcdé123"), "ABCDé123");
    }
}
