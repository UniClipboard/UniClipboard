use std::io::IsTerminal;

use serde::Serialize;
use uc_daemon_contract::api::dto::member::{
    ChooseDeviceGroupRequestDto, DeviceGroupChoiceIssueDto, DeviceGroupChoiceOptionDto,
    DeviceGroupChoiceOutcomeDto, DeviceGroupChoiceResultDto, DeviceGroupChoicesDto,
    DeviceMembershipDto,
};

use crate::commands::app_session::connect_facade_with_lease;
use crate::exit_codes;
use crate::{output, ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionError {
    NoIssues,
    IssueRequired,
    IssueChanged,
    NoChoices,
    ChoiceRequired,
    ChoiceChanged,
}

fn select_issue<'a>(
    state: &'a DeviceGroupChoicesDto,
    expected_issue: Option<&str>,
) -> Result<&'a DeviceGroupChoiceIssueDto, SelectionError> {
    match expected_issue {
        Some(expected) => state
            .issues
            .iter()
            .find(|issue| issue.issue_id == expected)
            .ok_or(SelectionError::IssueChanged),
        None if state.issues.is_empty() => Err(SelectionError::NoIssues),
        None if state.issues.len() == 1 => Ok(&state.issues[0]),
        None => Err(SelectionError::IssueRequired),
    }
}

fn select_choice<'a>(
    issue: &'a DeviceGroupChoiceIssueDto,
    expected_choice: Option<&str>,
) -> Result<&'a DeviceGroupChoiceOptionDto, SelectionError> {
    match expected_choice {
        Some(expected) => issue
            .choices
            .iter()
            .find(|choice| choice.choice_id == expected)
            .ok_or(SelectionError::ChoiceChanged),
        None if issue.choices.is_empty() => Err(SelectionError::NoChoices),
        None if issue.choices.len() == 1 => Ok(&issue.choices[0]),
        None => Err(SelectionError::ChoiceRequired),
    }
}

fn choice_removes_local_device(choice: &DeviceGroupChoiceOptionDto, local_device_id: &str) -> bool {
    !choice
        .member_device_ids
        .iter()
        .any(|device_id| device_id == local_device_id)
}

#[derive(Serialize)]
struct ChoiceOutput<'a> {
    ok: bool,
    result: &'a DeviceGroupChoiceResultDto,
    state: &'a DeviceGroupChoicesDto,
}

#[derive(Serialize)]
struct TrustErrorOutput<'a> {
    ok: bool,
    code: &'a str,
    message: &'a str,
    current_issue_id: Option<&'a str>,
}

pub async fn status(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Device groups");
    }
    let (_lease, service) = match connect_facade_with_lease(verbose).await {
        Ok(session) => session,
        Err(code) => return code,
    };
    let state = match service.query_device_group_choices().await {
        Ok(state) => state,
        Err(error) => {
            return emit_error(
                json,
                "device_group_choices_unavailable",
                &format!(
                    "Failed to query device groups: {}",
                    crate::commands::daemon_error_message(&error)
                ),
                None,
            );
        }
    };
    emit_status(&state, json)
}

pub async fn choose(
    requested_issue: Option<String>,
    requested_choice: Option<String>,
    confirm_local_removal: bool,
    json: bool,
    verbose: bool,
) -> i32 {
    if !json {
        ui::header("Choose device group");
    }
    let (_lease, service) = match connect_facade_with_lease(verbose).await {
        Ok(session) => session,
        Err(code) => return code,
    };
    let state = match service.query_device_group_choices().await {
        Ok(state) => state,
        Err(error) => {
            return emit_error(
                json,
                "device_group_choices_unavailable",
                &format!(
                    "Failed to query device groups: {}",
                    crate::commands::daemon_error_message(&error)
                ),
                None,
            );
        }
    };

    let interactive = !json && std::io::stderr().is_terminal();
    if interactive {
        render_status(&state);
    }
    let issue_input = match requested_issue {
        Some(issue) => Some(issue),
        None if interactive && state.issues.len() > 1 => match ui::input("Issue ID", false) {
            Ok(issue) => Some(issue),
            Err(error) => {
                return emit_error(false, "selection_failed", &error, None);
            }
        },
        None => None,
    };
    let issue = match select_issue(&state, issue_input.as_deref()) {
        Ok(issue) => issue,
        Err(error) => return emit_selection_error(json, error, None),
    };
    let choice_input = match requested_choice {
        Some(choice) => Some(choice),
        None if interactive && issue.choices.len() > 1 => match ui::input("Choice ID", false) {
            Ok(choice) => Some(choice),
            Err(error) => {
                return emit_error(false, "selection_failed", &error, Some(&issue.issue_id));
            }
        },
        None => None,
    };
    let choice = match select_choice(issue, choice_input.as_deref()) {
        Ok(choice) => choice,
        Err(error) => return emit_selection_error(json, error, Some(&issue.issue_id)),
    };

    let mut confirm_local_removal = confirm_local_removal;
    if choice_removes_local_device(choice, &state.device_trust.local_device_id)
        && !confirm_local_removal
    {
        if !interactive {
            return emit_error(
                json,
                "local_removal_confirmation_required",
                "This choice removes this device; pass --confirm-local-removal.",
                Some(&issue.issue_id),
            );
        }
        match ui::confirm(
            "This choice removes this device from the space. Continue?",
            false,
        ) {
            Ok(true) => confirm_local_removal = true,
            Ok(false) => {
                ui::end("No device group choice was made.");
                return exit_codes::EXIT_SUCCESS;
            }
            Err(error) => {
                return emit_error(false, "confirmation_failed", &error, Some(&issue.issue_id));
            }
        }
    }

    let request = ChooseDeviceGroupRequestDto {
        issue_id: issue.issue_id.clone(),
        choice_id: choice.choice_id.clone(),
        expected_revision: state.revision,
        confirm_local_removal,
    };
    let result = match service.choose_device_group(&request).await {
        Ok(result) => result,
        Err(error) => {
            return emit_error(
                json,
                "device_group_choice_failed",
                &format!(
                    "Failed to choose device group: {}",
                    crate::commands::daemon_error_message(&error)
                ),
                Some(&request.issue_id),
            );
        }
    };
    let latest = match service.query_device_group_choices().await {
        Ok(state) => state,
        Err(error) => {
            return emit_error(
                json,
                "device_group_refresh_failed",
                &format!(
                    "Choice was submitted, but current state could not be read: {}",
                    crate::commands::daemon_error_message(&error)
                ),
                Some(&request.issue_id),
            );
        }
    };

    emit_choice(&result, &latest, json)
}

pub(crate) fn emit_status(state: &DeviceGroupChoicesDto, json: bool) -> i32 {
    if json {
        return output::emit_json(state, "device group choices");
    }
    render_status(state);
    exit_codes::EXIT_SUCCESS
}

fn render_status(state: &DeviceGroupChoicesDto) {
    ui::info("revision", &state.revision.to_string());
    ui::info("local_device_id", &state.device_trust.local_device_id);
    ui::info(
        "local_membership",
        membership_label(state.device_trust.local_membership),
    );
    ui::info("pending_issues", &state.issues.len().to_string());
    for issue in &state.issues {
        ui::bar();
        ui::info("issue_id", &issue.issue_id);
        for choice in &issue.choices {
            let group = if choice.is_current_group {
                "current"
            } else {
                "candidate"
            };
            ui::info(
                "choice",
                &format!(
                    "{} ({group}; members={}; re_pairing={}; complete={})",
                    choice.choice_id,
                    choice.member_device_ids.join(","),
                    choice.requires_re_pairing,
                    choice.members_complete
                ),
            );
        }
    }
}

fn emit_choice(
    result: &DeviceGroupChoiceResultDto,
    state: &DeviceGroupChoicesDto,
    json: bool,
) -> i32 {
    let success = !matches!(
        result.outcome,
        DeviceGroupChoiceOutcomeDto::StateChanged
            | DeviceGroupChoiceOutcomeDto::LocalDeviceConfirmationRequired
    );
    let exit_code = if success {
        exit_codes::EXIT_SUCCESS
    } else {
        exit_codes::EXIT_ERROR
    };
    if json {
        return output::emit_json_with_code(
            &ChoiceOutput {
                ok: success,
                result,
                state,
            },
            "device group choice",
            exit_code,
        );
    }
    match result.outcome {
        DeviceGroupChoiceOutcomeDto::Completed => ui::success("Device group choice completed."),
        DeviceGroupChoiceOutcomeDto::Pending => {
            ui::warn("Device group choice is saved and still being completed.")
        }
        DeviceGroupChoiceOutcomeDto::RePairingRequired => {
            ui::warn("Device group changed; affected devices must be paired again.")
        }
        DeviceGroupChoiceOutcomeDto::AlreadyCompleted => {
            ui::success("Device group choice was already completed.")
        }
        DeviceGroupChoiceOutcomeDto::StateChanged => {
            ui::error("Device group state changed; review the latest choices.")
        }
        DeviceGroupChoiceOutcomeDto::LocalDeviceConfirmationRequired => {
            ui::error("This choice requires explicit local removal confirmation.")
        }
    }
    render_status(state);
    exit_code
}

fn emit_selection_error(json: bool, error: SelectionError, current_issue_id: Option<&str>) -> i32 {
    let (code, message) = match error {
        SelectionError::NoIssues => (
            "no_device_group_issues",
            "There are no device group issues.",
        ),
        SelectionError::IssueRequired => (
            "issue_id_required",
            "Multiple issues are available; pass --issue with an ID from status.",
        ),
        SelectionError::IssueChanged => (
            "device_group_state_changed",
            "The requested issue is no longer current; review status.",
        ),
        SelectionError::NoChoices => (
            "no_device_group_choices",
            "The selected issue has no available choices.",
        ),
        SelectionError::ChoiceRequired => (
            "choice_id_required",
            "Multiple choices are available; pass --choice with an ID from status.",
        ),
        SelectionError::ChoiceChanged => (
            "device_group_state_changed",
            "The requested choice is no longer current; review status.",
        ),
    };
    emit_error(json, code, message, current_issue_id)
}

fn membership_label(value: DeviceMembershipDto) -> &'static str {
    match value {
        DeviceMembershipDto::Active => "active",
        DeviceMembershipDto::Removed => "removed",
        DeviceMembershipDto::Unavailable => "unavailable",
        DeviceMembershipDto::Unknown => "unknown",
    }
}

fn emit_error(json: bool, code: &str, message: &str, current_issue_id: Option<&str>) -> i32 {
    if json {
        output::emit_json_with_code(
            &TrustErrorOutput {
                ok: false,
                code,
                message,
                current_issue_id,
            },
            "device group error",
            exit_codes::EXIT_ERROR,
        )
    } else {
        ui::error(message);
        exit_codes::EXIT_ERROR
    }
}

#[cfg(test)]
mod tests {
    use super::{choice_removes_local_device, select_choice, select_issue, SelectionError};
    use uc_daemon_contract::api::dto::member::{
        DeviceGroupChoiceIssueDto, DeviceGroupChoiceOptionDto, DeviceGroupChoicesDto,
        DeviceMembershipDto, DeviceTrustSnapshotDto,
    };

    fn choices() -> DeviceGroupChoicesDto {
        DeviceGroupChoicesDto {
            revision: 7,
            device_trust: DeviceTrustSnapshotDto {
                revision: 7,
                local_device_id: "local".to_string(),
                local_membership: DeviceMembershipDto::Active,
                current_change: None,
                current_join: None,
                pending_inbound_member: None,
                devices: vec![],
                recovery: "not_available_in_this_version".to_string(),
                allowed_actions: vec![],
                blocked_reason: None,
                updated_at_ms: 1,
            },
            issues: vec![DeviceGroupChoiceIssueDto {
                issue_id: "c:issue-1".to_string(),
                choices: vec![
                    DeviceGroupChoiceOptionDto {
                        choice_id: "b:current".to_string(),
                        is_current_group: true,
                        requires_re_pairing: false,
                        member_device_ids: vec!["local".to_string(), "peer-a".to_string()],
                        members_complete: true,
                    },
                    DeviceGroupChoiceOptionDto {
                        choice_id: "b:other".to_string(),
                        is_current_group: false,
                        requires_re_pairing: true,
                        member_device_ids: vec!["peer-b".to_string()],
                        members_complete: true,
                    },
                ],
            }],
        }
    }

    #[test]
    fn a_single_issue_and_choice_can_be_selected_by_opaque_id() {
        let state = choices();
        let issue = select_issue(&state, Some("c:issue-1")).expect("select issue");
        let choice = select_choice(issue, Some("b:other")).expect("select choice");

        assert_eq!(choice.choice_id, "b:other");
    }

    #[test]
    fn stale_issue_id_never_falls_through_to_current_issue() {
        let state = choices();
        assert_eq!(
            select_issue(&state, Some("c:stale")),
            Err(SelectionError::IssueChanged)
        );
    }

    #[test]
    fn a_choice_that_excludes_the_local_device_requires_confirmation() {
        let state = choices();
        let issue = select_issue(&state, Some("c:issue-1")).expect("select issue");

        assert!(!choice_removes_local_device(
            &issue.choices[0],
            &state.device_trust.local_device_id
        ));
        assert!(choice_removes_local_device(
            &issue.choices[1],
            &state.device_trust.local_device_id
        ));
    }
}
