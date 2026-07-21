use uc_engine::{
    ContentTypesPatch, ContentTypesSummary, CreateSpaceInput, DeviceSummary,
    EncryptionStateSummary, EngineConfig, EngineError, EngineErrorCategory, EngineEvent,
    EngineState, EntrySummary, ExportEntryInput, HostFileHandle, JoinSpaceInput,
    LocalDeviceSummary, MemberSyncPreferencesPatch, MemberSyncPreferencesSummary,
    MigrationPhaseSummary, MigrationProgressSummary, Operation, OperationKind, OperationResult,
    QueryHistoryInput, QueryMemberSyncPreferencesInput, RecoverSessionInput, RefreshReason,
    RemoveMemberInput, ResendEntryInput, SearchEntriesInput, SearchPageSummary,
    SearchResultSummary, SecretString, SendFilesInput, SendImageInput, SendTextInput,
    SetupInvitationSummary, SetupStateSummary, StorageStatsSummary, UnlockSpaceInput,
    UpdateMemberSyncPreferencesInput,
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
            Operation::SearchEntries(SearchEntriesInput {
                query: "private query".into(),
                operator: None,
                time_preset: None,
                from_ms: None,
                to_ms: None,
                content_types: None,
                extensions: None,
                source_devices: None,
                tags: None,
                limit: 50,
                offset: 0,
            }),
            OperationKind::SearchEntries,
        ),
        (Operation::QuerySearchTags, OperationKind::QuerySearchTags),
        (
            Operation::QuerySearchStatus,
            OperationKind::QuerySearchStatus,
        ),
        (
            Operation::RebuildSearchIndex,
            OperationKind::RebuildSearchIndex,
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
fn history_management_contract_preserves_results_without_debugging_user_content() {
    let operations = [
        (
            uc_engine::Operation::ListHistoryEntries(uc_engine::ListHistoryEntriesInput {
                limit: 50,
                offset: 0,
            }),
            uc_engine::OperationKind::ListHistoryEntries,
        ),
        (
            uc_engine::Operation::GetHistoryEntry(uc_engine::HistoryEntryInput {
                entry_id: "entry-1".into(),
            }),
            uc_engine::OperationKind::GetHistoryEntry,
        ),
        (
            uc_engine::Operation::DeleteHistoryEntry(uc_engine::HistoryEntryInput {
                entry_id: "entry-1".into(),
            }),
            uc_engine::OperationKind::DeleteHistoryEntry,
        ),
        (
            uc_engine::Operation::SetHistoryEntryFavorite(
                uc_engine::SetHistoryEntryFavoriteInput {
                    entry_id: "entry-1".into(),
                    is_favorited: true,
                },
            ),
            uc_engine::OperationKind::SetHistoryEntryFavorite,
        ),
        (
            uc_engine::Operation::QueryHistoryStats,
            uc_engine::OperationKind::QueryHistoryStats,
        ),
        (
            uc_engine::Operation::GetHistoryEntryResource(uc_engine::HistoryEntryInput {
                entry_id: "entry-1".into(),
            }),
            uc_engine::OperationKind::GetHistoryEntryResource,
        ),
        (
            uc_engine::Operation::ClearHistory,
            uc_engine::OperationKind::ClearHistory,
        ),
    ];
    for (operation, expected) in operations {
        assert_eq!(operation.kind(), expected);
    }

    let results = [
        uc_engine::OperationResult::HistoryEntries(vec![uc_engine::HistoryEntrySummary {
            entry_id: "entry-1".into(),
            preview: "private preview".into(),
            has_detail: true,
            size_bytes: 10,
            captured_at_ms: 1,
            content_type: "text".into(),
            thumbnail_url: Some("http://private/thumbnail".into()),
            is_encrypted: true,
            is_favorited: true,
            updated_at_ms: 2,
            active_time_ms: 3,
            file_transfer_status: None,
            file_transfer_reason: None,
            content_tags: vec!["private-tag".into()],
            link_urls: Some(vec!["https://private.example/secret".into()]),
            link_domains: Some(vec!["private.example".into()]),
            file_sizes: Some(vec![10]),
            image_width: None,
            image_height: None,
            payload_state: None,
        }]),
        uc_engine::OperationResult::HistoryEntry(uc_engine::HistoryEntryDetailSummary {
            entry_id: "entry-1".into(),
            content: "private full content".into(),
            size_bytes: 20,
            created_at_ms: 1,
            active_time_ms: 2,
            mime_type: Some("text/plain".into()),
        }),
        uc_engine::OperationResult::HistoryEntryResource(uc_engine::HistoryEntryResourceSummary {
            blob_id: Some("blob-1".into()),
            mime_type: Some("text/plain".into()),
            size_bytes: 20,
            url: Some("http://private/resource".into()),
            inline_data: Some(b"private inline content".to_vec()),
        }),
    ];
    let debug = format!("{results:?}");
    for secret in [
        "private preview",
        "private-tag",
        "private.example",
        "private full content",
        "private/resource",
        "private inline content",
    ] {
        assert!(!debug.contains(secret), "debug output leaked {secret}");
    }
}

#[test]
fn receive_progress_and_cancellation_have_stable_operations_and_results() {
    let operations = [
        (
            uc_engine::Operation::QueryEntryReceiveProgress(uc_engine::EntryReceiveProgressInput {
                entry_id: "entry-1".into(),
            }),
            uc_engine::OperationKind::QueryEntryReceiveProgress,
        ),
        (
            uc_engine::Operation::ListEntryReceiveProgress,
            uc_engine::OperationKind::ListEntryReceiveProgress,
        ),
        (
            uc_engine::Operation::CancelEntryReceive(uc_engine::CancelEntryReceiveInput {
                entry_id: "entry-1".into(),
                attempt_id: "attempt-1".into(),
            }),
            uc_engine::OperationKind::CancelEntryReceive,
        ),
        (
            uc_engine::Operation::CancelInboundTransfer(uc_engine::CancelInboundTransferInput {
                transfer_id: "transfer-1".into(),
                reason: uc_engine::TransferCancellationReason::LocalUser,
            }),
            uc_engine::OperationKind::CancelInboundTransfer,
        ),
    ];
    for (operation, expected) in operations {
        assert_eq!(operation.kind(), expected);
    }

    let progress = uc_engine::ReceiveProgressSummary {
        entry_id: "entry-1".into(),
        attempt_id: "attempt-1".into(),
        state: "transferring".into(),
        total_bytes: 100,
        completed_bytes: 40,
        items_total: 2,
        items_completed: 1,
    };
    assert_eq!(
        uc_engine::OperationResult::EntryReceiveProgress(Some(progress.clone())),
        uc_engine::OperationResult::EntryReceiveProgress(Some(progress.clone()))
    );
    assert_eq!(
        uc_engine::OperationResult::EntryReceiveProgressList(vec![progress]),
        uc_engine::OperationResult::EntryReceiveProgressList(vec![
            uc_engine::ReceiveProgressSummary {
                entry_id: "entry-1".into(),
                attempt_id: "attempt-1".into(),
                state: "transferring".into(),
                total_bytes: 100,
                completed_bytes: 40,
                items_total: 2,
                items_completed: 1,
            },
        ])
    );

    let receive_outcomes = [
        uc_engine::EntryReceiveCancellationOutcome::CancellationRequested,
        uc_engine::EntryReceiveCancellationOutcome::Cancelled,
        uc_engine::EntryReceiveCancellationOutcome::NotReceiving,
        uc_engine::EntryReceiveCancellationOutcome::TooLate,
        uc_engine::EntryReceiveCancellationOutcome::AlreadyTerminal,
        uc_engine::EntryReceiveCancellationOutcome::Superseded,
    ];
    assert_eq!(receive_outcomes.len(), 6);
    let transfer_outcomes = [
        uc_engine::InboundTransferCancellationOutcome::Cancelled,
        uc_engine::InboundTransferCancellationOutcome::NotInflight,
    ];
    assert_eq!(transfer_outcomes.len(), 2);
}

#[test]
fn search_contract_preserves_fields_without_debugging_user_content() {
    let input = SearchEntriesInput {
        query: "private search query".into(),
        operator: Some("and".into()),
        time_preset: None,
        from_ms: None,
        to_ms: None,
        content_types: Some("text".into()),
        extensions: Some("private-extension".into()),
        source_devices: Some("device-1".into()),
        tags: Some("private-tag".into()),
        limit: 50,
        offset: 0,
    };
    let input_debug = format!("{input:?}");
    assert!(!input_debug.contains("private search query"));
    assert!(!input_debug.contains("private-extension"));
    assert!(!input_debug.contains("private-tag"));

    let page = OperationResult::SearchPage(SearchPageSummary {
        total: 1,
        has_more: false,
        state: "ready".into(),
        items: vec![SearchResultSummary {
            entry_id: "entry-1".into(),
            content_type: "text".into(),
            active_time_ms: 42,
            tags: vec!["private-tag".into()],
            text_preview: Some("private preview".into()),
            char_count: Some(15),
            mime_type: "text/plain".into(),
            file_extensions: vec!["private-extension".into()],
            file_names: vec!["private-name.txt".into()],
            file_paths: vec!["/private/path/private-name.txt".into()],
            link_urls: vec!["https://private.example/secret".into()],
            source_device: Some("device-1".into()),
            payload_state: None,
        }],
    });
    let page_debug = format!("{page:?}");
    assert!(page_debug.contains("search_page"));
    assert!(!page_debug.contains("private preview"));
    assert!(!page_debug.contains("private-name.txt"));
    assert!(!page_debug.contains("private.example"));
    assert!(!page_debug.contains("private-tag"));
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
