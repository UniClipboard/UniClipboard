use uc_engine::{
    CreateSpaceInput, DeviceSummary, EngineConfig, EngineError, EngineErrorCategory, EngineEvent,
    EngineState, EntrySummary, ExportEntryInput, HostFileHandle, JoinSpaceInput, Operation,
    OperationKind, OperationResult, QueryHistoryInput, RecoverSessionInput, RefreshReason,
    ResendEntryInput, SecretString, SendFilesInput, SendImageInput, SendTextInput,
    UnlockSpaceInput,
};

#[test]
fn engine_config_has_stable_profile_and_version_inputs() {
    let config = EngineConfig::new("1.2.3").with_profile_id("private-profile-name");

    assert_eq!(config.app_version(), "1.2.3");
    assert_eq!(config.profile_id(), "private-profile-name");
    assert_eq!(EngineConfig::new("1.2.3").profile_id(), "default");

    let debug = format!("{config:?}");
    assert!(debug.contains("1.2.3"));
    assert!(!debug.contains("private-profile-name"));
}

#[test]
fn every_public_operation_has_a_stable_kind() {
    let operations = [
        (
            Operation::CreateSpace(CreateSpaceInput {
                device_name: "desktop".into(),
                passphrase: SecretString::new("secret"),
                passphrase_confirmation: SecretString::new("secret"),
            }),
            OperationKind::CreateSpace,
        ),
        (
            Operation::JoinSpace(JoinSpaceInput {
                invitation_code: "ABCD-EFGH".into(),
                device_name: "mobile".into(),
                passphrase: SecretString::new("secret"),
            }),
            OperationKind::JoinSpace,
        ),
        (
            Operation::UnlockSpace(UnlockSpaceInput {
                passphrase: SecretString::new("secret"),
            }),
            OperationKind::UnlockSpace,
        ),
        (
            Operation::RecoverSession(RecoverSessionInput {
                allow_secure_storage_unlock: true,
            }),
            OperationKind::RecoverSession,
        ),
        (Operation::IssueInvitation, OperationKind::IssueInvitation),
        (Operation::ListDevices, OperationKind::ListDevices),
        (
            Operation::SendText(SendTextInput {
                text: "private text".into(),
                target_devices: vec!["phone".into()],
            }),
            OperationKind::SendText,
        ),
        (
            Operation::SendImage(SendImageInput {
                bytes: vec![1, 2, 3],
                mime_type: "image/png".into(),
                target_devices: vec![],
            }),
            OperationKind::SendImage,
        ),
        (
            Operation::SendFiles(SendFilesInput {
                files: vec![HostFileHandle::new("host-file-1")],
                target_devices: vec![],
            }),
            OperationKind::SendFiles,
        ),
        (
            Operation::QueryHistory(QueryHistoryInput {
                cursor: None,
                limit: 50,
                query: Some("private query".into()),
            }),
            OperationKind::QueryHistory,
        ),
        (
            Operation::ExportEntry(ExportEntryInput {
                entry_id: "entry-1".into(),
                destination: HostFileHandle::new("export-target-1"),
            }),
            OperationKind::ExportEntry,
        ),
        (
            Operation::ResendEntry(ResendEntryInput {
                entry_id: "entry-1".into(),
                target_devices: vec![],
            }),
            OperationKind::ResendEntry,
        ),
    ];

    for (operation, expected) in operations {
        assert_eq!(operation.kind(), expected);
    }
}

#[test]
fn sensitive_operation_debug_output_is_redacted() {
    let operation = Operation::SendText(SendTextInput {
        text: "never-print-this".into(),
        target_devices: vec!["phone".into()],
    });
    let debug = format!("{operation:?}");

    assert!(!debug.contains("never-print-this"));
    assert!(debug.contains("send_text"));
}

#[test]
fn setup_input_debug_output_redacts_user_and_pairing_data() {
    let input = JoinSpaceInput {
        invitation_code: "NEVER-SHOW".into(),
        device_name: "Private Phone".into(),
        passphrase: SecretString::new("never-show-passphrase"),
    };
    let debug = format!("{input:?}");

    assert!(!debug.contains("NEVER-SHOW"));
    assert!(!debug.contains("Private Phone"));
    assert!(!debug.contains("never-show-passphrase"));
    assert!(debug.contains("[REDACTED]"));
}

#[test]
fn lifecycle_contract_rejects_operations_outside_running_state() {
    assert!(EngineState::Running.accepts_operations());
    for state in [
        EngineState::Quiescing,
        EngineState::Quiesced,
        EngineState::Suspended,
        EngineState::ShuttingDown,
        EngineState::Stopped,
    ] {
        assert!(!state.accepts_operations(), "{state:?} accepted work");
    }
}

#[test]
fn lifecycle_contract_only_allows_documented_transitions() {
    assert!(EngineState::Running.can_transition_to(EngineState::Quiescing));
    assert!(EngineState::Quiescing.can_transition_to(EngineState::Quiesced));
    assert!(EngineState::Quiesced.can_transition_to(EngineState::Suspended));
    assert!(EngineState::Suspended.can_transition_to(EngineState::Running));
    assert!(EngineState::Running.can_transition_to(EngineState::ShuttingDown));
    assert!(EngineState::Suspended.can_transition_to(EngineState::ShuttingDown));
    assert!(EngineState::ShuttingDown.can_transition_to(EngineState::Stopped));

    assert!(!EngineState::Running.can_transition_to(EngineState::Suspended));
    assert!(!EngineState::Stopped.can_transition_to(EngineState::Running));
    assert!(!EngineState::Quiescing.can_transition_to(EngineState::Running));
}

#[test]
fn public_errors_expose_only_stable_classification() {
    let error = EngineError::new(1201, EngineErrorCategory::Unavailable, true);
    assert_eq!(error.code(), 1201);
    assert_eq!(error.category(), EngineErrorCategory::Unavailable);
    assert!(error.is_retryable());
    assert_eq!(error.to_string(), "engine error 1201 (unavailable)");
}

#[test]
fn lagged_consumers_receive_a_refresh_event() {
    let event = EngineEvent::RefreshRequired {
        reason: RefreshReason::ConsumerLagged,
    };
    assert_eq!(event.kind(), "refresh_required");
}

#[test]
fn operation_result_debug_output_redacts_user_content() {
    let results = [
        OperationResult::InvitationIssued {
            invitation_code: "NEVER-SHOW-INVITATION".into(),
        },
        OperationResult::Devices(vec![DeviceSummary {
            device_id: "device-1".into(),
            display_name: "Private Phone Name".into(),
            online: true,
        }]),
        OperationResult::HistoryPage {
            entries: vec![EntrySummary {
                entry_id: "entry-1".into(),
                content_type: "text".into(),
                preview: Some("private clipboard preview".into()),
                created_at_ms: 1,
            }],
            next_cursor: Some("private-cursor".into()),
        },
    ];

    let debug = format!("{results:?}");
    for secret in [
        "NEVER-SHOW-INVITATION",
        "Private Phone Name",
        "private clipboard preview",
        "private-cursor",
    ] {
        assert!(!debug.contains(secret), "debug output leaked {secret}");
    }
}

#[test]
fn device_summary_debug_output_redacts_display_name() {
    let device = DeviceSummary {
        device_id: "device-1".into(),
        display_name: "Private Phone Name".into(),
        online: true,
    };

    let debug = format!("{device:?}");
    assert!(!debug.contains("Private Phone Name"));
    assert!(debug.contains("device-1"));
    assert!(debug.contains("online"));
}
