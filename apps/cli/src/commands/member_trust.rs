use std::io::IsTerminal;

use serde::Serialize;
use uc_daemon_contract::api::dto::member::{
    DecideDeviceTrustRequestDto, DeviceMembershipDto, DeviceTrustChangeDto, DeviceTrustChoiceDto,
    DeviceTrustDecisionDto, DeviceTrustImpactDto, DeviceTrustSnapshotDto,
};

use crate::commands::app_session::connect_facade_with_lease;
use crate::exit_codes;
use crate::{output, ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectChangeError {
    NoCurrentChange,
    ExpectedChangeRequired,
    StateChanged,
}

fn select_change<'a>(
    current: Option<&'a DeviceTrustChangeDto>,
    expected_change: Option<&str>,
    interactive: bool,
) -> Result<&'a DeviceTrustChangeDto, SelectChangeError> {
    let current = current.ok_or(SelectChangeError::NoCurrentChange)?;
    match expected_change {
        Some(expected) if expected != current.change_id => Err(SelectChangeError::StateChanged),
        Some(_) => Ok(current),
        None if interactive => Ok(current),
        None => Err(SelectChangeError::ExpectedChangeRequired),
    }
}

fn requires_local_removal_confirmation(change: &DeviceTrustChangeDto) -> bool {
    change.apply_impact.local_device_outcome == DeviceMembershipDto::Removed
}

fn choice_is_allowed(change: &DeviceTrustChangeDto, choice: DeviceTrustChoiceDto) -> bool {
    change.allowed_choices.contains(&choice)
}

#[derive(Serialize)]
struct TrustStatusOutput<'a> {
    ok: bool,
    status: &'static str,
    revision: u64,
    local_device_id: &'a str,
    local_membership: DeviceMembershipDto,
    current_change: Option<TrustChangeOutput<'a>>,
}

#[derive(Serialize)]
struct TrustChangeOutput<'a> {
    change_id: &'a str,
    proposed_by_device_id: &'a str,
    target_device_ids: &'a [String],
    includes_local_device: bool,
    apply_impact: TrustImpactOutput<'a>,
    keep_current_impact: TrustImpactOutput<'a>,
    allowed_choices: &'a [DeviceTrustChoiceDto],
}

#[derive(Serialize)]
struct TrustImpactOutput<'a> {
    usable_device_ids: &'a [String],
    paused_device_ids: &'a [String],
    local_device_outcome: DeviceMembershipDto,
    requires_rejoin_device_ids: &'a [String],
}

#[derive(Serialize)]
struct TrustDecisionOutput<'a> {
    ok: bool,
    status: &'static str,
    change_id: Option<&'a str>,
    completed_choice: Option<DeviceTrustChoiceDto>,
    current_change_id: Option<&'a str>,
    snapshot: TrustStatusOutput<'a>,
}

#[derive(Serialize)]
struct TrustErrorOutput<'a> {
    ok: bool,
    code: &'a str,
    message: &'a str,
    current_change_id: Option<&'a str>,
}

pub async fn status(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Device trust");
    }
    let (_lease, service) = match connect_facade_with_lease(verbose).await {
        Ok(session) => session,
        Err(code) => return code,
    };
    let snapshot = match service.device_trust().await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return emit_error(
                json,
                "device_trust_unavailable",
                &format!(
                    "Failed to query device trust: {}",
                    crate::commands::daemon_error_message(&err)
                ),
                None,
            )
        }
    };
    emit_status(&snapshot, json)
}

pub async fn decide(
    choice: DeviceTrustChoiceDto,
    expected_change: Option<String>,
    confirm_local_removal: bool,
    json: bool,
    verbose: bool,
) -> i32 {
    if !json {
        ui::header(match choice {
            DeviceTrustChoiceDto::ApplyChange => "Apply device trust change",
            DeviceTrustChoiceDto::KeepCurrentDeviceGroup => "Keep current device group",
        });
    }
    let (_lease, service) = match connect_facade_with_lease(verbose).await {
        Ok(session) => session,
        Err(code) => return code,
    };
    let snapshot = match service.device_trust().await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return emit_error(
                json,
                "device_trust_unavailable",
                &format!(
                    "Failed to query device trust: {}",
                    crate::commands::daemon_error_message(&err)
                ),
                None,
            )
        }
    };
    let interactive = !json && std::io::stderr().is_terminal();
    let change = match select_change(
        snapshot.current_change.as_ref(),
        expected_change.as_deref(),
        interactive,
    ) {
        Ok(change) => change,
        Err(SelectChangeError::NoCurrentChange) => {
            return emit_error(
                json,
                "no_current_change",
                "There is no device trust change to decide.",
                None,
            )
        }
        Err(SelectChangeError::ExpectedChangeRequired) => {
            return emit_error(
                json,
                "change_id_required",
                "Non-interactive use requires --change <CHANGE-ID>.",
                snapshot
                    .current_change
                    .as_ref()
                    .map(|change| change.change_id.as_str()),
            )
        }
        Err(SelectChangeError::StateChanged) => {
            return emit_error(
                json,
                "device_trust_state_changed",
                "The device trust change has changed; review the current status.",
                snapshot
                    .current_change
                    .as_ref()
                    .map(|change| change.change_id.as_str()),
            )
        }
    };

    if !choice_is_allowed(change, choice) {
        return emit_error(
            json,
            "choice_not_allowed",
            "The selected decision is not available for this device trust change.",
            Some(&change.change_id),
        );
    }

    if interactive {
        render_change(change);
        let prompt = match choice {
            DeviceTrustChoiceDto::ApplyChange => "Apply this device group change?",
            DeviceTrustChoiceDto::KeepCurrentDeviceGroup => "Keep the current device group?",
        };
        match ui::confirm(prompt, false) {
            Ok(true) => {}
            Ok(false) => {
                ui::end("No device trust decision was made.");
                return exit_codes::EXIT_SUCCESS;
            }
            Err(err) => {
                return emit_error(false, "confirmation_failed", &err, Some(&change.change_id))
            }
        }
    }

    let mut confirm_local_removal = confirm_local_removal;
    if choice == DeviceTrustChoiceDto::ApplyChange
        && requires_local_removal_confirmation(change)
        && !confirm_local_removal
    {
        if !interactive {
            return emit_error(
                json,
                "local_removal_confirmation_required",
                "This change removes this device; pass --confirm-local-removal.",
                Some(&change.change_id),
            );
        }
        match ui::confirm(
            "This change removes this device from the space. Continue?",
            false,
        ) {
            Ok(true) => confirm_local_removal = true,
            Ok(false) => {
                ui::end("No device trust decision was made.");
                return exit_codes::EXIT_SUCCESS;
            }
            Err(err) => {
                return emit_error(false, "confirmation_failed", &err, Some(&change.change_id))
            }
        }
    }

    let request = DecideDeviceTrustRequestDto {
        change_id: change.change_id.clone(),
        choice,
        confirm_local_removal,
    };
    let decision = match service.decide_device_trust(&request).await {
        Ok(decision) => decision,
        Err(err) => {
            return emit_error(
                json,
                "device_trust_decision_failed",
                &format!(
                    "Failed to decide device trust: {}",
                    crate::commands::daemon_error_message(&err)
                ),
                Some(&request.change_id),
            )
        }
    };
    emit_decision(&decision, json)
}

fn emit_status(snapshot: &DeviceTrustSnapshotDto, json: bool) -> i32 {
    let output = status_output(snapshot);
    if json {
        return output::emit_json(&output, "device trust status");
    }
    ui::info("local_device_id", output.local_device_id);
    ui::info(
        "local_membership",
        membership_label(output.local_membership),
    );
    match snapshot.current_change.as_ref() {
        Some(change) => render_change(change),
        None => ui::info("status", "none"),
    }
    exit_codes::EXIT_SUCCESS
}

fn status_output(snapshot: &DeviceTrustSnapshotDto) -> TrustStatusOutput<'_> {
    TrustStatusOutput {
        ok: true,
        status: if snapshot.current_change.is_some() {
            "pending_change"
        } else {
            "none"
        },
        revision: snapshot.revision,
        local_device_id: &snapshot.local_device_id,
        local_membership: snapshot.local_membership,
        current_change: snapshot.current_change.as_ref().map(change_output),
    }
}

fn change_output(change: &DeviceTrustChangeDto) -> TrustChangeOutput<'_> {
    TrustChangeOutput {
        change_id: &change.change_id,
        proposed_by_device_id: &change.proposed_by_device_id,
        target_device_ids: &change.target_device_ids,
        includes_local_device: change.includes_local_device,
        apply_impact: impact_output(&change.apply_impact),
        keep_current_impact: impact_output(&change.keep_current_impact),
        allowed_choices: &change.allowed_choices,
    }
}

fn impact_output(impact: &DeviceTrustImpactDto) -> TrustImpactOutput<'_> {
    TrustImpactOutput {
        usable_device_ids: &impact.usable_device_ids,
        paused_device_ids: &impact.paused_device_ids,
        local_device_outcome: impact.local_device_outcome,
        requires_rejoin_device_ids: &impact.requires_rejoin_device_ids,
    }
}

fn render_change(change: &DeviceTrustChangeDto) {
    ui::info("change_id", &change.change_id);
    ui::info("proposed_by", &change.proposed_by_device_id);
    ui::info("targets", &change.target_device_ids.join(","));
    ui::info(
        "apply_local_outcome",
        membership_label(change.apply_impact.local_device_outcome),
    );
    ui::info(
        "apply_paused_devices",
        &change.apply_impact.paused_device_ids.len().to_string(),
    );
    ui::info(
        "keep_paused_devices",
        &change
            .keep_current_impact
            .paused_device_ids
            .len()
            .to_string(),
    );
}

fn emit_decision(decision: &DeviceTrustDecisionDto, json: bool) -> i32 {
    let (status, change_id, completed_choice, current_change_id, snapshot, exit_code) =
        match decision {
            DeviceTrustDecisionDto::Applied {
                change_id,
                snapshot,
            } => (
                "applied",
                Some(change_id.as_str()),
                None,
                None,
                snapshot,
                exit_codes::EXIT_SUCCESS,
            ),
            DeviceTrustDecisionDto::KeptCurrentDeviceGroup {
                change_id,
                snapshot,
            } => (
                "kept_current_device_group",
                Some(change_id.as_str()),
                None,
                None,
                snapshot,
                exit_codes::EXIT_SUCCESS,
            ),
            DeviceTrustDecisionDto::AlreadyCompleted {
                change_id,
                completed_choice,
                snapshot,
            } => (
                "already_completed",
                Some(change_id.as_str()),
                Some(*completed_choice),
                None,
                snapshot,
                exit_codes::EXIT_SUCCESS,
            ),
            DeviceTrustDecisionDto::StateChanged {
                current_change_id,
                snapshot,
            } => (
                "state_changed",
                None,
                None,
                current_change_id.as_deref(),
                snapshot,
                exit_codes::EXIT_ERROR,
            ),
            DeviceTrustDecisionDto::LocalDeviceConfirmationRequired {
                change_id,
                snapshot,
            } => (
                "local_device_confirmation_required",
                Some(change_id.as_str()),
                None,
                None,
                snapshot,
                exit_codes::EXIT_ERROR,
            ),
        };
    let output = TrustDecisionOutput {
        ok: exit_code == exit_codes::EXIT_SUCCESS,
        status,
        change_id,
        completed_choice,
        current_change_id,
        snapshot: status_output(snapshot),
    };
    if json {
        return output::emit_json_with_code(&output, "device trust decision", exit_code);
    }
    if exit_code == exit_codes::EXIT_SUCCESS {
        ui::success(match status {
            "applied" => "Device group change applied.",
            "kept_current_device_group" => "Current device group kept.",
            _ => "Device trust decision was already completed.",
        });
    } else {
        ui::error(match status {
            "state_changed" => "Device trust state changed; review the current status.",
            _ => "This decision requires explicit local removal confirmation.",
        });
    }
    exit_code
}

fn membership_label(value: DeviceMembershipDto) -> &'static str {
    match value {
        DeviceMembershipDto::Active => "active",
        DeviceMembershipDto::Removed => "removed",
        DeviceMembershipDto::Unavailable => "unavailable",
        DeviceMembershipDto::Unknown => "unknown",
    }
}

fn emit_error(json: bool, code: &str, message: &str, current_change_id: Option<&str>) -> i32 {
    if json {
        output::emit_json_with_code(
            &TrustErrorOutput {
                ok: false,
                code,
                message,
                current_change_id,
            },
            "device trust error",
            exit_codes::EXIT_ERROR,
        )
    } else {
        ui::error(message);
        exit_codes::EXIT_ERROR
    }
}

#[cfg(test)]
mod tests {
    use super::{
        choice_is_allowed, requires_local_removal_confirmation, select_change, status_output,
        SelectChangeError,
    };
    use uc_daemon_contract::api::dto::member::{
        DeviceMembershipDto, DeviceTrustChangeDto, DeviceTrustChoiceDto, DeviceTrustImpactDto,
    };

    fn change(id: &str, removes_local: bool) -> DeviceTrustChangeDto {
        let local_outcome = if removes_local {
            DeviceMembershipDto::Removed
        } else {
            DeviceMembershipDto::Active
        };
        DeviceTrustChangeDto {
            change_id: id.to_string(),
            proposed_by_device_id: "sponsor".to_string(),
            target_device_ids: vec!["device-b".to_string()],
            includes_local_device: removes_local,
            apply_impact: DeviceTrustImpactDto {
                usable_device_ids: vec![],
                paused_device_ids: vec![],
                local_device_outcome: local_outcome,
                requires_rejoin_device_ids: vec![],
            },
            keep_current_impact: DeviceTrustImpactDto {
                usable_device_ids: vec![],
                paused_device_ids: vec![],
                local_device_outcome: DeviceMembershipDto::Active,
                requires_rejoin_device_ids: vec![],
            },
            allowed_choices: vec![
                DeviceTrustChoiceDto::ApplyChange,
                DeviceTrustChoiceDto::KeepCurrentDeviceGroup,
            ],
            blocked_reason: None,
        }
    }

    #[test]
    fn non_interactive_decision_requires_expected_change_id() {
        let current = change("change-1", false);
        assert_eq!(
            select_change(Some(&current), None, false),
            Err(SelectChangeError::ExpectedChangeRequired)
        );
    }

    #[test]
    fn stale_change_id_never_falls_through_to_current_change() {
        let current = change("change-2", false);
        assert_eq!(
            select_change(Some(&current), Some("change-1"), false),
            Err(SelectChangeError::StateChanged)
        );
    }

    #[test]
    fn interactive_decision_can_bind_to_displayed_current_change() {
        let current = change("change-1", false);
        assert_eq!(
            select_change(Some(&current), None, true)
                .expect("interactive current change")
                .change_id,
            "change-1"
        );
    }

    #[test]
    fn applying_local_removal_requires_separate_confirmation() {
        assert!(requires_local_removal_confirmation(&change(
            "change-1", true
        )));
        assert!(!requires_local_removal_confirmation(&change(
            "change-1", false
        )));
    }

    #[test]
    fn trust_decision_must_be_allowed_by_the_current_change() {
        let mut current = change("change-1", false);
        current.allowed_choices = vec![DeviceTrustChoiceDto::KeepCurrentDeviceGroup];

        assert!(!choice_is_allowed(
            &current,
            DeviceTrustChoiceDto::ApplyChange
        ));
        assert!(choice_is_allowed(
            &current,
            DeviceTrustChoiceDto::KeepCurrentDeviceGroup
        ));
    }

    #[test]
    fn device_trust_json_shape_includes_current_change_and_impacts() {
        let current_change = change("change-1", false);
        let snapshot = uc_daemon_contract::api::dto::member::DeviceTrustSnapshotDto {
            revision: 7,
            local_device_id: "device-local".to_string(),
            local_membership: DeviceMembershipDto::Active,
            current_change: Some(current_change),
            current_join: None,
            pending_inbound_member: None,
            devices: vec![],
            recovery: "not_available_in_this_version".to_string(),
            allowed_actions: vec![],
            blocked_reason: None,
            updated_at_ms: 10,
        };
        let value =
            serde_json::to_value(status_output(&snapshot)).expect("serialize device trust status");
        assert_eq!(value["current_change"]["change_id"], "change-1");
        assert_eq!(
            value["current_change"]["apply_impact"]["local_device_outcome"],
            "active"
        );
        assert!(value.get("currentChange").is_none());
    }
}
