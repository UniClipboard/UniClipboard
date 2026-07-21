use uc_engine::{
    ContentTypesPatch, ContentTypesSummary, CreateSpaceInput, DeviceSummary,
    EncryptionStateSummary, EngineConfig, EngineError, EngineErrorCategory, EngineEvent,
    EngineState, EntrySummary, ExportEntryInput, HostFileHandle, JoinSpaceInput,
    LocalDeviceSummary, MemberSyncPreferencesPatch, MemberSyncPreferencesSummary,
    MigrationPhaseSummary, MigrationProgressSummary, Operation, OperationKind, OperationResult,
    QueryHistoryInput, QueryMemberSyncPreferencesInput, RecoverSessionInput, RefreshReason,
    RemoveMemberInput, ResendEntryInput, SecretString, SendFilesInput, SendImageInput,
    SendTextInput, SetupInvitationSummary, SetupStateSummary, StorageStatsSummary,
    UnlockSpaceInput, UpdateMemberSyncPreferencesInput,
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
                device_name: Some("desktop".into()),
                passphrase: SecretString::new("secret"),
                passphrase_confirmation: SecretString::new("secret"),
            }),
            OperationKind::CreateSpace,
        ),
        (
            Operation::JoinSpace(JoinSpaceInput {
                invitation_code: "ABCD-EFGH".into(),
                device_name: Some("mobile".into()),
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
        (Operation::CancelInvitation, OperationKind::CancelInvitation),
        (Operation::ResetSpace, OperationKind::ResetSpace),
        (
            Operation::FactoryResetSpace,
            OperationKind::FactoryResetSpace,
        ),
        (Operation::QuerySetupState, OperationKind::QuerySetupState),
        (
            Operation::QueryMigrationProgress,
            OperationKind::QueryMigrationProgress,
        ),
        (
            Operation::QueryStorageStats,
            OperationKind::QueryStorageStats,
        ),
        (
            Operation::ClearStorageCache,
            OperationKind::ClearStorageCache,
        ),
        (Operation::QueryLocalDevice, OperationKind::QueryLocalDevice),
        (
            Operation::QueryEncryptionState,
            OperationKind::QueryEncryptionState,
        ),
        (Operation::LockEncryption, OperationKind::LockEncryption),
        (
            Operation::VerifySecureStorageAccess,
            OperationKind::VerifySecureStorageAccess,
        ),
        (Operation::ListDevices, OperationKind::ListDevices),
        (
            Operation::QueryMemberSyncPreferences(QueryMemberSyncPreferencesInput {
                device_id: "member-1".into(),
            }),
            OperationKind::QueryMemberSyncPreferences,
        ),
        (
            Operation::UpdateMemberSyncPreferences(UpdateMemberSyncPreferencesInput {
                device_id: "member-1".into(),
                patch: MemberSyncPreferencesPatch::default(),
            }),
            OperationKind::UpdateMemberSyncPreferences,
        ),
        (
            Operation::RemoveMember(RemoveMemberInput {
                device_id: "member-1".into(),
            }),
            OperationKind::RemoveMember,
        ),
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
fn member_sync_preferences_preserve_partial_updates_and_stable_results() {
    let patch = MemberSyncPreferencesPatch {
        send_enabled: Some(false),
        receive_enabled: None,
        send_content_types: Some(ContentTypesPatch {
            text: Some(true),
            ..Default::default()
        }),
        receive_content_types: None,
    };
    assert_eq!(patch.send_enabled, Some(false));
    assert!(patch.receive_enabled.is_none());
    assert_eq!(
        patch.send_content_types.as_ref().and_then(|p| p.text),
        Some(true)
    );
    assert!(patch.receive_content_types.is_none());

    let preferences = OperationResult::MemberSyncPreferences(MemberSyncPreferencesSummary {
        send_enabled: false,
        receive_enabled: true,
        send_content_types: ContentTypesSummary {
            text: true,
            image: false,
            link: false,
            file: false,
            code_snippet: false,
            rich_text: false,
        },
        receive_content_types: ContentTypesSummary {
            text: true,
            image: true,
            link: true,
            file: true,
            code_snippet: true,
            rich_text: true,
        },
    });

    assert!(format!("{preferences:?}").contains("member_sync_preferences"));
    assert!(format!("{:?}", OperationResult::MemberRemoved).contains("member_removed"));
}

#[test]
fn encryption_operations_expose_only_stable_state_and_outcomes() {
    let state = OperationResult::EncryptionState(EncryptionStateSummary {
        initialized: true,
        session_ready: false,
    });
    let locked = OperationResult::EncryptionLocked;
    let access = OperationResult::SecureStorageAccess { granted: true };
    let factory_reset = OperationResult::SpaceFactoryReset;

    assert!(format!("{state:?}").contains("encryption_state"));
    assert!(format!("{locked:?}").contains("encryption_locked"));
    assert!(format!("{access:?}").contains("secure_storage_access"));
    assert!(format!("{factory_reset:?}").contains("space_factory_reset"));
}

#[test]
fn local_device_result_redacts_the_display_name() {
    let result = OperationResult::LocalDevice(LocalDeviceSummary {
        device_id: "device-1".into(),
        display_name: "Private MacBook".into(),
    });
    let debug = format!("{result:?}");

    assert!(debug.contains("local_device"));
    assert!(debug.contains("device-1"));
    assert!(!debug.contains("Private MacBook"));
}

#[test]
fn storage_results_expose_counts_without_host_paths() {
    let stats = OperationResult::StorageStats(StorageStatsSummary {
        total_bytes: 50,
        database_bytes: 10,
        vault_bytes: 20,
        cache_bytes: 15,
        logs_bytes: 5,
    });
    let cleared = OperationResult::StorageCacheCleared { freed_bytes: 15 };

    assert!(format!("{stats:?}").contains("storage_stats"));
    assert!(format!("{cleared:?}").contains("storage_cache_cleared"));
}

#[test]
fn cancel_invitation_has_a_stable_terminal_result() {
    assert_eq!(
        OperationResult::InvitationCancelled,
        OperationResult::InvitationCancelled
    );
}

#[test]
fn reset_space_has_a_stable_terminal_result() {
    assert_eq!(OperationResult::SpaceReset, OperationResult::SpaceReset);
}

#[test]
fn setup_state_result_preserves_invitation_and_redacts_user_content() {
    let result = OperationResult::SetupState(SetupStateSummary {
        has_completed: true,
        current_invitation: Some(SetupInvitationSummary {
            invitation_code: "NEVER-SHOW".into(),
            expires_at_ms: 1234,
        }),
        device_name: Some("Private Device".into()),
    });
    let debug = format!("{result:?}");

    assert!(!debug.contains("NEVER-SHOW"));
    assert!(!debug.contains("Private Device"));
    assert!(debug.contains("setup_state"));
}

#[test]
fn migration_progress_result_exposes_only_coarse_phase_and_count() {
    let result = OperationResult::MigrationProgress(MigrationProgressSummary {
        phase: Some(MigrationPhaseSummary::HandshakeDone),
        backup_record_count: 42,
    });

    assert!(matches!(
        result,
        OperationResult::MigrationProgress(MigrationProgressSummary {
            phase: Some(MigrationPhaseSummary::HandshakeDone),
            backup_record_count: 42,
        })
    ));
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
        device_name: Some("Private Phone".into()),
        passphrase: SecretString::new("never-show-passphrase"),
    };
    let debug = format!("{input:?}");

    assert!(!debug.contains("NEVER-SHOW"));
    assert!(!debug.contains("Private Phone"));
    assert!(!debug.contains("never-show-passphrase"));
    assert!(debug.contains("[REDACTED]"));
}

#[test]
fn create_space_contract_supports_saved_device_name_and_returns_identity() {
    let input = CreateSpaceInput {
        device_name: None,
        passphrase: SecretString::new("never-show-passphrase"),
        passphrase_confirmation: SecretString::new("never-show-passphrase"),
    };
    assert!(input.device_name.is_none());

    let result = OperationResult::SpaceCreated {
        space_id: "space-1".into(),
        self_device_id: "device-1".into(),
        identity_fingerprint: "fingerprint-1".into(),
    };
    assert!(matches!(
        result,
        OperationResult::SpaceCreated {
            ref space_id,
            ref self_device_id,
            ref identity_fingerprint,
        } if space_id == "space-1"
            && self_device_id == "device-1"
            && identity_fingerprint == "fingerprint-1"
    ));
}

#[test]
fn join_space_contract_supports_saved_device_name_and_returns_both_identities() {
    let input = JoinSpaceInput {
        invitation_code: "NEVER-SHOW".into(),
        device_name: None,
        passphrase: SecretString::new("never-show-passphrase"),
    };
    assert!(input.device_name.is_none());

    let result = OperationResult::SpaceJoined {
        sponsor_device_id: "sponsor-1".into(),
        sponsor_identity_fingerprint: "sponsor-fingerprint".into(),
        space_id: "space-1".into(),
        self_device_id: "device-1".into(),
        self_identity_fingerprint: "self-fingerprint".into(),
        migrated_records: Some(42),
    };
    assert!(matches!(
        result,
        OperationResult::SpaceJoined {
            ref sponsor_device_id,
            ref sponsor_identity_fingerprint,
            ref space_id,
            ref self_device_id,
            ref self_identity_fingerprint,
            migrated_records: Some(42),
        } if sponsor_device_id == "sponsor-1"
            && sponsor_identity_fingerprint == "sponsor-fingerprint"
            && space_id == "space-1"
            && self_device_id == "device-1"
            && self_identity_fingerprint == "self-fingerprint"
    ));
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
            expires_at_ms: 1,
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
