//! Explicit user confirmations for scenarios with concurrent membership updates.

use std::time::Duration;

use serde_json::Value;

use crate::CapturedOutput;

/// History exchange can advance the membership revision without changing the
/// removal decision. Simulate reviewing and confirming that decision. A conflict
/// is not success: review the same facts again before another user submission.
/// Transport errors, changed decisions, and non-conflict failures are terminal.
/// The CLI itself must continue to submit only once per invocation.
pub async fn confirm_device_group(
    mut run: impl FnMut(&[&str]) -> CapturedOutput,
    expected_issue: &Value,
    expected_choice: &Value,
    confirm_local_removal: bool,
    timeout: Duration,
) -> Result<Value, String> {
    let issue_id = expected_issue["issueId"]
        .as_str()
        .ok_or("missing issue ID")?;
    let choice_id = expected_choice["choiceId"]
        .as_str()
        .ok_or("missing choice ID")?;
    let mut args = vec![
        "--json", "member", "trust", "choose", "--issue", issue_id, "--choice", choice_id,
    ];
    if confirm_local_removal {
        args.push("--confirm-local-removal");
    }
    let deadline = tokio::time::Instant::now() + timeout;
    let mut minimum_revision = 0;
    loop {
        let current = run(&["--json", "member", "trust", "status"]);
        if !current.success() {
            return Err(format!(
                "read choices before confirmation failed: {current:?}"
            ));
        }
        let current = parse(&current)?;
        review(&current, expected_issue, expected_choice)?;
        let reviewed_revision = revision(&current)?;
        if reviewed_revision < minimum_revision {
            return Err("reviewed revision regressed after conflict".into());
        }

        let output = run(&args);
        let result = parse(&output)?;
        match result["result"]["outcome"].as_str() {
            Some("state_changed") => {
                if output.exit_code != 1 || result["ok"] != false {
                    return Err(format!("conflict must remain a CLI failure: {output:?}"));
                }
                let conflict_revision = result["result"]["currentRevision"]
                    .as_u64()
                    .ok_or("conflict omitted currentRevision")?;
                if conflict_revision <= reviewed_revision
                    || revision(&result["state"])? < conflict_revision
                {
                    return Err(format!("conflict did not advance revision: {result}"));
                }
                // Check both the conflict response and the next fresh read. A
                // changed removal/impact must never become an automatic consent.
                review(&result["state"], expected_issue, expected_choice)?;
                minimum_revision = revision(&result["state"])?;
                if tokio::time::Instant::now() >= deadline {
                    return Err(format!("device group never settled: {result}"));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
                if tokio::time::Instant::now() >= deadline {
                    return Err("device group confirmation deadline expired".into());
                }
            }
            Some("completed" | "pending" | "re_pairing_required" | "already_completed")
                if output.success() && result["ok"] == true =>
            {
                return Ok(result)
            }
            _ => return Err(format!("device group confirmation failed: {output:?}")),
        }
    }
}

fn parse(output: &CapturedOutput) -> Result<Value, String> {
    serde_json::from_str(output.stdout.trim())
        .map_err(|error| format!("invalid confirmation JSON ({error}): {output:?}"))
}

fn revision(state: &Value) -> Result<u64, String> {
    state["revision"]
        .as_u64()
        .ok_or_else(|| "missing revision".into())
}

fn review(state: &Value, expected_issue: &Value, expected_choice: &Value) -> Result<(), String> {
    let issue = state["issues"]
        .as_array()
        .and_then(|issues| {
            issues
                .iter()
                .find(|issue| issue["issueId"] == expected_issue["issueId"])
        })
        .ok_or("same issue must remain current")?;
    let choice = issue["choices"]
        .as_array()
        .and_then(|choices| {
            choices
                .iter()
                .find(|choice| choice["choiceId"] == expected_choice["choiceId"])
        })
        .ok_or("same choice must remain available")?;
    if choice["membersComplete"] != true
        || issue["reason"]["detailsComplete"] != true
        || issue["reason"] != expected_issue["reason"]
        || decision(choice) != decision(expected_choice)
    {
        return Err(format!(
            "device group decision facts changed or are incomplete: expected reason={}, current reason={}; expected choice={expected_choice}, current choice={choice}",
            expected_issue["reason"], issue["reason"],
        ));
    }
    Ok(())
}

/// These three lists describe current peer readiness, not the membership being
/// chosen (Engine's pending_impact derives them from sync_state/confirmed_position).
/// Keep all other fields, including rejoin requirements and local removal, strict.
fn decision(choice: &Value) -> Value {
    let mut decision = choice.clone();
    if let Some(impact) = decision["impact"].as_object_mut() {
        for field in [
            "syncScopeDeviceIds",
            "pausedDeviceIds",
            "pendingConfirmationDeviceIds",
        ] {
            impact.remove(field);
        }
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    fn state(revision: u64) -> Value {
        json!({"revision": revision, "issues": [{
            "issueId": "p:removal", "reason": {
                "kind": "pending_removal", "detailsComplete": true,
                "changes": [{"actor": "alice", "target": "bob"}]
            }, "choices": [{
                "choiceId": "apply", "isCurrentGroup": false,
                "membersComplete": true, "memberDeviceIds": ["alice", "carol"],
                "requiresRePairing": false,
                "impact": {"localDeviceOutcome": "active", "requiresRejoinDeviceIds": ["bob"]}
            }]
        }]})
    }

    fn output(exit_code: i32, value: Value) -> CapturedOutput {
        CapturedOutput {
            exit_code,
            stdout: value.to_string(),
            stderr: String::new(),
        }
    }

    fn conflict() -> CapturedOutput {
        output(
            1,
            json!({"ok": false, "result": {
            "outcome": "state_changed", "currentRevision": 9
        }, "state": state(9)}),
        )
    }

    async fn scripted(
        script: Vec<(&str, CapturedOutput)>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let expected = state(8);
        let issue = &expected["issues"][0];
        let choice = &issue["choices"][0];
        let mut script = VecDeque::from(script);
        let result = confirm_device_group(
            |args| {
                let (command, output) =
                    script.pop_front().expect("unexpected extra CLI invocation");
                assert_eq!(args[3], command);
                if command == "choose" {
                    assert_eq!(&args[4..], &["--issue", "p:removal", "--choice", "apply"]);
                }
                output
            },
            issue,
            choice,
            false,
            timeout,
        )
        .await;
        assert!(script.is_empty(), "script not fully exercised");
        result
    }

    #[tokio::test]
    async fn conflict_requires_fresh_review_and_successful_confirmation() {
        let result = scripted(
            vec![
                ("status", output(0, state(8))),
                ("choose", conflict()),
                ("status", output(0, state(9))),
                (
                    "choose",
                    output(0, json!({"ok": true, "result": {"outcome": "completed"}})),
                ),
            ],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(result["result"]["outcome"], "completed");
    }

    #[tokio::test]
    async fn peer_readiness_changes_do_not_change_the_selected_membership() {
        let mut current = state(8);
        current["issues"][0]["choices"][0]["impact"]["syncScopeDeviceIds"] = json!(["alice"]);
        let mut conflict_value = parse(&conflict()).unwrap();
        conflict_value["state"]["issues"][0]["choices"][0]["impact"]["pausedDeviceIds"] =
            json!(["alice"]);
        let mut refreshed = state(9);
        refreshed["issues"][0]["choices"][0]["impact"]["pendingConfirmationDeviceIds"] =
            json!(["alice"]);
        scripted(
            vec![
                ("status", output(0, current)),
                ("choose", output(1, conflict_value)),
                ("status", output(0, refreshed)),
                (
                    "choose",
                    output(0, json!({"ok": true, "result": {"outcome": "completed"}})),
                ),
            ],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn changed_impact_in_conflict_is_not_reconfirmed() {
        let mut value = parse(&conflict()).unwrap();
        value["state"]["issues"][0]["choices"][0]["impact"]["requiresRejoinDeviceIds"] =
            json!(["alice"]);
        assert!(scripted(
            vec![
                ("status", output(0, state(8))),
                ("choose", output(1, value)),
            ],
            Duration::from_secs(5)
        )
        .await
        .unwrap_err()
        .contains("facts changed"));
    }

    #[tokio::test]
    async fn changed_facts_on_fresh_read_stop_before_another_submission() {
        let mut changed = state(9);
        changed["issues"][0]["reason"]["changes"] = json!([{"target": "alice"}]);
        assert!(scripted(
            vec![
                ("status", output(0, state(8))),
                ("choose", conflict()),
                ("status", output(0, changed)),
            ],
            Duration::from_secs(5)
        )
        .await
        .unwrap_err()
        .contains("facts changed"));
    }

    #[tokio::test]
    async fn unrelated_failure_is_terminal() {
        for value in [
            json!({"ok": false, "code": "device_group_choice_failed"}),
            json!({"ok": false, "result": {"outcome": "local_device_confirmation_required"}}),
        ] {
            assert!(scripted(
                vec![
                    ("status", output(0, state(8))),
                    ("choose", output(1, value)),
                ],
                Duration::from_secs(5)
            )
            .await
            .is_err());
        }
    }

    #[tokio::test]
    async fn conflict_without_revision_progress_is_terminal() {
        assert!(scripted(
            vec![("status", output(0, state(9))), ("choose", conflict()),],
            Duration::from_secs(5)
        )
        .await
        .unwrap_err()
        .contains("advance revision"));
    }

    #[tokio::test]
    async fn persistent_conflict_hits_deadline() {
        assert!(scripted(
            vec![("status", output(0, state(8))), ("choose", conflict()),],
            Duration::ZERO
        )
        .await
        .unwrap_err()
        .contains("never settled"));
    }

    #[tokio::test]
    async fn fresh_read_cannot_regress_from_the_conflict_response() {
        let mut value = parse(&conflict()).unwrap();
        value["state"] = state(10);
        assert!(scripted(
            vec![
                ("status", output(0, state(8))),
                ("choose", output(1, value)),
                ("status", output(0, state(9))),
            ],
            Duration::from_secs(5)
        )
        .await
        .unwrap_err()
        .contains("regressed"));
    }

    #[tokio::test]
    async fn conflict_must_not_be_reported_as_success() {
        let value = parse(&conflict()).unwrap();
        assert!(scripted(
            vec![
                ("status", output(0, state(8))),
                ("choose", output(0, value)),
            ],
            Duration::from_secs(5)
        )
        .await
        .unwrap_err()
        .contains("CLI failure"));
    }

    #[test]
    fn review_rejects_missing_or_incomplete_decisions() {
        let expected = state(8);
        let issue = &expected["issues"][0];
        let choice = &issue["choices"][0];
        for pointer in [
            "/issues/0/issueId",
            "/issues/0/choices/0/choiceId",
            "/issues/0/choices/0/membersComplete",
            "/issues/0/reason/detailsComplete",
        ] {
            let mut changed = state(9);
            *changed.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(review(&changed, issue, choice).is_err(), "{pointer}");
        }
    }
}
