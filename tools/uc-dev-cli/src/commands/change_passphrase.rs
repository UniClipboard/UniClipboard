//! `uniclip space change-passphrase` — replace the space passphrase via the daemon.
//!
//! Distinct from `space reset` (rebuild, passphrase kept) and `space init`
//! (first-run only). The daemon only accepts the change for an unlocked,
//! single-device space, so no current passphrase is asked for.

use serde::Serialize;

use crate::commands::app_session::connect_facade_with_lease;
use crate::commands::daemon_error_message;
use crate::exit_codes;
use crate::output;
use crate::ui;

pub struct ChangePassphraseArgs {
    pub passphrase: Option<String>,
}

#[derive(Serialize)]
struct ChangePassphraseResult {
    ok: bool,
    status: &'static str,
}

/// Next step to suggest for a daemon rejection code, if one applies.
fn rejection_hint(code: Option<&str>) -> Option<&'static str> {
    match code {
        Some("MULTIPLE_DEVICES") => Some(
            "Remove the other devices with `uniclip member remove`, or rebuild with `uniclip space reset --yes`, then retry.",
        ),
        Some("SPACE_LOCKED") => Some("Unlock this device first, then retry."),
        _ => None,
    }
}

pub async fn run(args: ChangePassphraseArgs, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Change space passphrase");
    }

    let passphrase = match args.passphrase {
        Some(ref p) if p.trim().is_empty() => {
            ui::error("--passphrase is empty");
            return exit_codes::EXIT_ERROR;
        }
        Some(p) => p,
        None if json => {
            ui::error("--passphrase is required in --json mode");
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password_with_confirm("New space passphrase", "Confirm passphrase") {
            Ok(p) if p.trim().is_empty() => {
                ui::error("Passphrase cannot be empty");
                return exit_codes::EXIT_ERROR;
            }
            Ok(p) => p,
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    let (_lease, service) = match connect_facade_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if let Err(error) = service
        .change_encryption_passphrase(&passphrase, &passphrase)
        .await
    {
        ui::error(&format!(
            "Failed to change passphrase: {}",
            daemon_error_message(&error)
        ));
        let code = error
            .downcast_ref::<uc_daemon_client::DaemonRequestError>()
            .and_then(uc_daemon_client::DaemonRequestError::code);
        if let (false, Some(hint)) = (json, rejection_hint(code)) {
            ui::info("hint", hint);
        }
        return exit_codes::EXIT_ERROR;
    }

    if json {
        output::emit_json(
            &ChangePassphraseResult {
                ok: true,
                status: "changed",
            },
            "passphrase change result",
        )
    } else {
        ui::success("Passphrase changed. Local history was kept.");
        ui::info(
            "note",
            "Unused pairing invitations were invalidated; issue a new one with `uniclip space invite`.",
        );
        exit_codes::EXIT_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::{rejection_hint, ChangePassphraseResult};

    #[test]
    fn json_output_uses_stable_identifiers() {
        let value = serde_json::to_value(ChangePassphraseResult {
            ok: true,
            status: "changed",
        })
        .expect("passphrase change output should serialize");

        assert_eq!(
            value,
            serde_json::json!({ "ok": true, "status": "changed" })
        );
    }

    #[test]
    fn hints_cover_only_actionable_rejections() {
        assert!(rejection_hint(Some("MULTIPLE_DEVICES")).is_some());
        assert!(rejection_hint(Some("SPACE_LOCKED")).is_some());
        assert!(rejection_hint(Some("RECOVERY_REQUIRED")).is_none());
        assert!(rejection_hint(None).is_none());
    }
}
