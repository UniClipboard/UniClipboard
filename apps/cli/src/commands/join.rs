//! `uniclip join` — join a space via daemon HTTP API. The route is chosen by
//! explicit intent, not by the device's setup state.
//!
//! * Default (no `--switch`) → calls `POST /v2/setup/redeem` (joiner side of
//!   Slice 1 pairing). A single blocking RPC — the daemon drives the dial/wait
//!   loop internally, so we simply await the result. Safe to run when already
//!   in the *same* space: stale rows are replaced in the new handshake (issue
//!   #1023), so this is also the re-pair-after-unpair path.
//! * `--switch` → calls `POST /v2/setup/switch-space`, which drives the
//!   4-phase re-encryption migration internally for moving to a *different*
//!   space. This is destructive, so we confirm first (unless `--yes`) and show
//!   a spinner while it runs.
//!
//! Routing on explicit intent (rather than a local setup-state check) keeps
//! same-space re-pair non-destructive: a set-up device re-joining its own
//! space must redeem, not migrate. Both paths handle Ctrl+C for clean
//! cancellation.

use tokio::select;
use tokio::signal;

use uc_daemon_client::DaemonClientContext;
use uc_daemon_contract::api::dto::settings::{GeneralSettingsPatchDto, SettingsPatchDto};
use uc_daemon_contract::api::dto::v2::setup::{RedeemRequest, SwitchSpaceRequest};

use crate::commands::app_session::{
    connect_with_lease, default_device_name, ensure_daemon_for_setup,
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
    pub switch: bool,
    pub yes: bool,
}

pub async fn run(args: JoinArgs, verbose: bool) -> i32 {
    ui::header("Join a space");

    // Collect invitation code: --code wins; otherwise prompt. Shared by both
    // the redeem and switch paths (both are rendezvous lookup keys, so both
    // get the same byte-for-byte normalization).
    let code_str = match args.code {
        Some(c) if !c.trim().is_empty() => normalize_invitation_code(&c),
        Some(_) => {
            ui::error("--code is empty");
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password("Invitation code") {
            Ok(c) if !c.trim().is_empty() => normalize_invitation_code(&c),
            Ok(_) => {
                ui::error("Invitation code cannot be empty");
                return exit_codes::EXIT_ERROR;
            }
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    // Collect passphrase (single entry, no confirmation). Shared by both paths.
    let passphrase_str = match args.passphrase {
        Some(p) if !p.trim().is_empty() => p,
        Some(_) => {
            ui::error("--passphrase is empty");
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password("Space passphrase") {
            Ok(p) if !p.trim().is_empty() => p,
            Ok(_) => {
                ui::error("Passphrase cannot be empty");
                return exit_codes::EXIT_ERROR;
            }
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    // Route by explicit intent, not by setup state. Without `--switch` we
    // always take the non-destructive redeem path — which doubles as the
    // re-pair-after-unpair path (issue #1023), since redeeming an invitation
    // for the space this device is already in just replaces stale rows. Only
    // `--switch` opts into the destructive re-encryption migration to a
    // different space.
    if args.switch {
        if args.device_name.is_some() {
            ui::warn("--device-name is ignored when switching spaces");
        }
        return run_switch(code_str, passphrase_str, args.yes, verbose).await;
    }

    run_redeem(code_str, passphrase_str, args.device_name, verbose).await
}

/// First-time join: redeem an invitation and adopt the sponsor's space.
async fn run_redeem(
    code_str: String,
    passphrase_str: String,
    device_name_arg: Option<String>,
    verbose: bool,
) -> i32 {
    // Determine device name.
    let device_name = device_name_arg
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(default_device_name);
    let device_name = match device_name {
        Some(n) => n,
        None => {
            ui::error("Device name is required (pass --device-name or set a system hostname)");
            return exit_codes::EXIT_ERROR;
        }
    };

    // Ensure daemon is running (no setup gate — we ARE the setup command).
    let service = match ensure_daemon_for_setup(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };
    let _lease = match service.hold_control_lease().await {
        Ok(g) => g,
        Err(err) => {
            ui::error(&format!("Failed to acquire control lease: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(c) => c,
        Err(err) => {
            ui::error(&format!("Failed to build daemon client context: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    // Set device name via settings BEFORE redeem — RedeemRequest has no
    // device_name field; the daemon reads it from persisted settings.
    let patch = SettingsPatchDto {
        general: Some(GeneralSettingsPatchDto {
            device_name: Some(Some(device_name.clone())),
            ..Default::default()
        }),
        ..Default::default()
    };
    if let Err(err) = ctx.settings_client().update_settings(patch).await {
        ui::warn(&format!("Failed to set device name: {err}"));
        // non-fatal — redeem might still work with hostname default
    }

    let spinner = ui::spinner("Dialing sponsor and running handshake...");
    let req = RedeemRequest {
        code: code_str,
        passphrase: passphrase_str,
    };

    let setup_client = ctx.setup_v2_client();
    let redeem_fut = setup_client.redeem_invitation(&req);
    tokio::pin!(redeem_fut);

    select! {
        result = &mut redeem_fut => match result {
            Ok(resp) => {
                ui::spinner_finish_success(&spinner, "Joined space");
                ui::info("space_id", &resp.space_id);
                ui::info("self_device_id", &resp.self_device_id);
                ui::info("self_device_name", &device_name);
                ui::info("self_fingerprint", &resp.self_identity_fingerprint);
                ui::info("sponsor_device_id", &resp.sponsor_device_id);
                ui::info("sponsor_fingerprint", &resp.sponsor_identity_fingerprint);
                exit_codes::EXIT_SUCCESS
            }
            Err(err) => {
                ui::spinner_finish_error(
                    &spinner,
                    &format!("Join failed: {}", crate::commands::daemon_error_message(&err)),
                );
                exit_codes::EXIT_ERROR
            }
        },
        _ = signal::ctrl_c() => {
            ui::spinner_finish_error(&spinner, "Interrupted by user");
            EXIT_SIGINT
        }
    }
}

/// Already-set-up device: switch to another sponsor's space, re-encrypting
/// local clipboard history under the new master key (4-phase migration).
///
/// Destructive, so we confirm first unless `--yes` was passed. The daemon
/// drives the migration internally and persists `MigrationStatePort`, so a
/// crash mid-run auto-resumes on the next `uniclip` invocation.
async fn run_switch(code_str: String, new_passphrase: String, yes: bool, verbose: bool) -> i32 {
    ui::warn(
        "This device is already in a space. Switching will re-encrypt all local \
         clipboard history under the new space's master key.",
    );
    if !yes {
        match ui::confirm("Switch to the new space now?", false) {
            Ok(true) => {}
            Ok(false) => {
                ui::end("Cancelled — staying in the current space.");
                return exit_codes::EXIT_SUCCESS;
            }
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        }
    }

    // Device IS set up → normal connect path (vs. redeem's setup-gated spawn).
    let (_lease, ctx) = match connect_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let spinner = ui::spinner(
        "Migrating local clipboard history to the new space (4 phases \u{2014} this may take a while)...",
    );

    let req = SwitchSpaceRequest {
        code: code_str,
        new_passphrase,
    };

    let setup_client = ctx.setup_v2_client();
    let switch_fut = setup_client.switch_space(&req);
    tokio::pin!(switch_fut);

    select! {
        result = &mut switch_fut => match result {
            Ok(resp) => {
                ui::spinner_finish_success(&spinner, "Switched space");
                ui::info("space_id", &resp.space_id);
                ui::info("self_device_id", &resp.self_device_id);
                ui::info("self_fingerprint", &resp.self_identity_fingerprint);
                ui::info("sponsor_device_id", &resp.sponsor_device_id);
                ui::info("sponsor_fingerprint", &resp.sponsor_identity_fingerprint);
                ui::info("migrated_records", &resp.migrated_records.to_string());
                exit_codes::EXIT_SUCCESS
            }
            Err(err) => {
                ui::spinner_finish_error(
                    &spinner,
                    &format!(
                        "Switch-space failed: {}",
                        crate::commands::daemon_error_message(&err)
                    ),
                );
                exit_codes::EXIT_ERROR
            }
        },
        _ = signal::ctrl_c() => {
            ui::spinner_finish_error(&spinner, "Interrupted by user");
            ui::info(
                "note",
                "Migration may be partially complete. Restart `uniclip` to auto-resume.",
            );
            EXIT_SIGINT
        }
    }
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
