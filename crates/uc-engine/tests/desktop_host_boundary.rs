use std::path::{Path, PathBuf};

#[test]
fn daemon_does_not_export_legacy_runtime_fields() {
    let daemon_source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/daemon/src");
    let violations = rust_sources(&daemon_source)
        .into_iter()
        .flat_map(|path| {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            source
                .lines()
                .enumerate()
                .filter_map(move |(index, line)| {
                    let line = line.trim_start();
                    let exports_app_facade = line.starts_with("pub app_facade:");
                    let exports_app_deps = line.starts_with("pub ") && line.contains("AppDeps");
                    (exports_app_facade || exports_app_deps)
                        .then(|| format!("{}:{}: {line}", path.display(), index + 1))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "daemon host must not export legacy runtime fields:\n{}",
        violations.join("\n")
    );
}

#[test]
fn daemon_run_loop_does_not_depend_on_legacy_application_runtime() {
    let run_loop =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/daemon/src/daemon/run_loop.rs");
    let source = std::fs::read_to_string(&run_loop)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", run_loop.display()));

    assert!(
        !source.contains("AppFacade") && !source.contains("AppDeps"),
        "daemon run loop must only own process run and shutdown ordering"
    );
}

#[test]
fn daemon_library_does_not_export_in_process_legacy_assembly() {
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/daemon/src/lib.rs");
    let source = std::fs::read_to_string(&library)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", library.display()));

    assert!(
        !source.contains("ProcessRuntimeHandles")
            && !source.contains("start_in_process")
            && !source.contains("ProcessRuntimeContext")
            && !source.contains("build_process_runtime"),
        "daemon library must not export legacy in-process assembly"
    );
}

#[test]
fn daemon_process_runtime_does_not_expose_task_registry() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/daemon/src/daemon/process_runtime.rs");
    let source = std::fs::read_to_string(&runtime)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", runtime.display()));

    assert!(
        !source.contains("fn task_registry("),
        "daemon process runtime must own background task registration"
    );
}

#[test]
fn daemon_process_runtime_does_not_expose_app_facade() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/daemon/src/daemon/process_runtime.rs");
    let source = std::fs::read_to_string(&runtime)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", runtime.display()));

    assert!(
        !source.contains("fn app_facade("),
        "daemon process runtime must expose behavior instead of the legacy facade"
    );
}

#[test]
fn daemon_startup_recovery_delegates_business_orchestration_to_engine() {
    let recovery = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/daemon/src/daemon/startup_recovery.rs");
    let source = std::fs::read_to_string(&recovery)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", recovery.display()));
    let forbidden = [
        "recover_encryption_session",
        "SpaceSetupFacade",
        "try_resume_session",
        "refresh_presence",
        "input.receive_readiness.ensure_receive_ready",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|token| source.contains(token))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "daemon startup recovery must leave business recovery to uc-engine; found: {}",
        violations.join(", ")
    );
    assert!(
        source.contains("execute_recover_session("),
        "daemon startup recovery must invoke the uc-engine recovery implementation"
    );
}

#[test]
fn daemon_invitation_handler_delegates_business_orchestration_to_engine() {
    let handler =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/v2/setup.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));

    assert!(
        !source.contains(".issue_pairing_invitation()")
            && !source.contains("IssuePairingInvitationError"),
        "daemon invitation HTTP handler must not own invitation business orchestration"
    );
    assert!(
        source.contains("execute_issue_invitation("),
        "daemon invitation HTTP handler must invoke the uc-engine invitation implementation"
    );
}

#[test]
fn daemon_create_space_handler_delegates_business_orchestration_to_engine() {
    let handler =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/v2/setup.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let initialize_handler = source
        .split("pub(crate) async fn initialize(")
        .nth(1)
        .and_then(|source| source.split("// POST /v2/setup/issue-invitation").next())
        .expect("create-space handler must remain discoverable");

    assert!(
        !initialize_handler.contains(".initialize_space(")
            && !initialize_handler.contains("InitializeSpaceError"),
        "daemon create-space handler must not own create-space business orchestration"
    );
    assert!(
        initialize_handler.contains("execute_create_space("),
        "daemon create-space handler must invoke the uc-engine create-space implementation"
    );
}

#[test]
fn daemon_join_space_handler_delegates_business_orchestration_to_engine() {
    let handler =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/v2/setup.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let redeem_handler = source
        .split("pub(crate) async fn redeem(")
        .nth(1)
        .and_then(|source| source.split("// POST /v2/setup/cancel").next())
        .expect("join-space handler must remain discoverable");

    assert!(
        !redeem_handler.contains(".redeem_pairing_invitation(")
            && !redeem_handler.contains("RedeemPairingInvitationError"),
        "daemon join-space handler must not own join-space business orchestration"
    );
    assert!(
        redeem_handler.contains("execute_join_space(")
            && redeem_handler.contains("JoinSpaceMode::Fresh"),
        "daemon join-space handler must invoke the uc-engine fresh-join implementation"
    );
}

#[test]
fn daemon_switch_space_handler_delegates_business_orchestration_to_engine() {
    let handler =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/v2/setup.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let switch_handler = source
        .split("pub(crate) async fn switch_space(")
        .nth(1)
        .and_then(|source| source.split("// GET /v2/setup/migration-progress").next())
        .expect("switch-space handler must remain discoverable");

    assert!(
        !switch_handler.contains(".switch_space(") && !switch_handler.contains("SwitchSpaceError"),
        "daemon switch-space handler must not own switch-space business orchestration"
    );
    assert!(
        switch_handler.contains("execute_join_space(")
            && switch_handler.contains("JoinSpaceMode::Switch"),
        "daemon switch-space handler must invoke the uc-engine switch implementation"
    );
}

#[test]
fn daemon_cancel_invitation_handler_delegates_business_orchestration_to_engine() {
    let handler =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/v2/setup.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let cancel_handler = source
        .split("pub(crate) async fn cancel(")
        .nth(1)
        .and_then(|source| source.split("// POST /v2/setup/reset").next())
        .expect("cancel-invitation handler must remain discoverable");

    assert!(
        !cancel_handler.contains(".cancel_invitation(")
            && !cancel_handler.contains("CancelInvitationError"),
        "daemon cancel-invitation handler must not own invitation business orchestration"
    );
    assert!(
        cancel_handler.contains("execute_cancel_invitation("),
        "daemon cancel-invitation handler must invoke the uc-engine implementation"
    );
}

#[test]
fn daemon_reset_space_handler_delegates_business_orchestration_to_engine() {
    let handler =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/v2/setup.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let reset_handler = source
        .split("pub(crate) async fn reset(")
        .nth(1)
        .and_then(|source| source.split("// GET /v2/setup/state").next())
        .expect("reset-space handler must remain discoverable");

    assert!(
        !reset_handler.contains(".reset(") && !reset_handler.contains("ResetSpaceError"),
        "daemon reset-space handler must not own reset business orchestration"
    );
    assert!(
        reset_handler.contains("execute_reset_space("),
        "daemon reset-space handler must invoke the uc-engine implementation"
    );
}

#[test]
fn daemon_setup_state_handler_delegates_business_orchestration_to_engine() {
    let handler =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/v2/setup.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let state_handler = source
        .split("pub(crate) async fn get_state(")
        .nth(1)
        .and_then(|source| source.split("// POST /v2/setup/switch-space").next())
        .expect("setup-state handler must remain discoverable");

    assert!(
        !state_handler.contains(".query_setup_state(")
            && !state_handler.contains("QuerySetupStateError"),
        "daemon setup-state handler must not own setup-state business orchestration"
    );
    assert!(
        state_handler.contains("execute_query_setup_state("),
        "daemon setup-state handler must invoke the uc-engine implementation"
    );
}

#[test]
fn daemon_migration_progress_handler_delegates_business_orchestration_to_engine() {
    let handler =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/v2/setup.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let progress_handler = source
        .split("pub(crate) async fn query_migration_progress(")
        .nth(1)
        .expect("migration-progress handler must remain discoverable");

    assert!(
        !progress_handler.contains(".query_migration_progress(")
            && !progress_handler.contains("QueryMigrationProgressError"),
        "daemon migration-progress handler must not own migration business orchestration"
    );
    assert!(
        progress_handler.contains("execute_query_migration_progress("),
        "daemon migration-progress handler must invoke the uc-engine implementation"
    );
}

#[test]
fn daemon_passphrase_unlock_delegates_business_orchestration_to_engine() {
    let handler = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/uc-webserver/src/api/encryption.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let unlock_handler = source
        .split("async fn unlock_with_passphrase_handler")
        .nth(1)
        .and_then(|source| source.split("fn broadcast_session_ready").next())
        .expect("passphrase unlock handler must remain discoverable");

    assert!(
        !source.contains("UnlockSpaceError")
            && !unlock_handler.contains(".unlock_space(")
            && !unlock_handler.contains("on_session_ready("),
        "daemon passphrase unlock must not own engine recovery or repeat session recovery"
    );
    assert!(
        unlock_handler.contains("execute_unlock_space(")
            && unlock_handler.contains("broadcast_session_ready("),
        "daemon passphrase unlock must invoke the engine and only broadcast success"
    );
}

#[test]
fn daemon_silent_unlock_delegates_business_orchestration_to_engine() {
    let handler = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/uc-webserver/src/api/encryption.rs");
    let source = std::fs::read_to_string(&handler)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler.display()));
    let unlock_handler = source
        .split("async fn unlock_handler(")
        .nth(1)
        .and_then(|source| {
            source
                .split("/// POST /encryption/unlock-with-passphrase")
                .next()
        })
        .expect("silent unlock handler must remain discoverable");

    assert!(
        !unlock_handler.contains("try_resume_session(")
            && !unlock_handler.contains("on_session_ready("),
        "daemon silent unlock must not own or repeat session recovery"
    );
    assert!(
        unlock_handler.contains("execute_recover_session(")
            && unlock_handler.contains("broadcast_session_ready("),
        "daemon silent unlock must invoke the engine and only broadcast success"
    );
}

#[test]
fn daemon_upgrade_paths_depend_on_the_desktop_upgrade_capability() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let startup_path = manifest.join("../../apps/daemon/src/daemon/startup_recovery.rs");
    let startup = std::fs::read_to_string(&startup_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", startup_path.display()));
    let startup_upgrade = startup
        .split("pub(crate) async fn record_upgrade_status_at_startup(")
        .nth(1)
        .and_then(|source| source.split("#[cfg(test)]").next())
        .expect("startup upgrade function must remain discoverable");

    assert!(
        !startup_upgrade.contains("AppFacade") && !startup_upgrade.contains("app_facade"),
        "daemon startup upgrade detection must depend on UpgradeFacade directly"
    );

    let handler_path = manifest.join("../../crates/uc-webserver/src/api/upgrade.rs");
    let handler = std::fs::read_to_string(&handler_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler_path.display()));

    assert!(
        !handler.contains("app_facade_or_error()") && handler.contains("state.upgrade"),
        "daemon upgrade HTTP handlers must use the desktop upgrade capability directly"
    );
}

#[test]
fn daemon_diagnostics_paths_depend_on_the_desktop_diagnostics_capability() {
    let handler_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/uc-webserver/src/api/diagnostics.rs");
    let handler = std::fs::read_to_string(&handler_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler_path.display()));

    assert!(
        !handler.contains("app_facade_or_error()") && handler.contains("state.diagnostics"),
        "daemon diagnostics HTTP handlers must use the desktop diagnostics capability directly"
    );
}

#[test]
fn daemon_config_paths_depend_on_the_desktop_config_migration_capability() {
    let handler_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/config.rs");
    let handler = std::fs::read_to_string(&handler_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler_path.display()));

    assert!(
        !handler.contains("app_facade_or_error()") && handler.contains("state.config_migration"),
        "daemon config HTTP handlers must use the desktop config migration capability directly"
    );
}

#[test]
fn daemon_settings_paths_depend_on_the_desktop_settings_capability() {
    let handler_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/settings.rs");
    let handler = std::fs::read_to_string(&handler_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler_path.display()));

    assert!(
        !handler.contains("app_facade_or_error()") && handler.contains("state.settings"),
        "daemon settings HTTP handlers must use the desktop settings capability directly"
    );
}

#[test]
fn daemon_mobile_lan_lifecycle_depends_only_on_mobile_sync_capability() {
    let lifecycle_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/daemon/src/daemon/mobile_lan_lifecycle.rs");
    let lifecycle = std::fs::read_to_string(&lifecycle_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", lifecycle_path.display()));

    assert!(
        !lifecycle.contains("AppFacade")
            && !lifecycle.contains("app_facade")
            && lifecycle.contains("MobileSyncFacade"),
        "mobile LAN lifecycle must use a dedicated mobile-sync capability slot"
    );
}

#[test]
fn daemon_mobile_sync_http_depends_on_the_mobile_sync_capability() {
    let handler_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/uc-webserver/src/api/mobile_sync.rs");
    let handler = std::fs::read_to_string(&handler_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler_path.display()));

    assert!(
        !handler.contains("app_facade_or_error()") && handler.contains("state.mobile_sync"),
        "mobile-sync HTTP handlers must use the dedicated compatibility capability"
    );
}

#[test]
fn daemon_storage_handlers_delegate_storage_ownership_to_engine() {
    let handler_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/storage.rs");
    let handler = std::fs::read_to_string(&handler_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler_path.display()));

    assert!(
        !handler.contains(".storage\n")
            && handler.contains("execute_query_storage_stats(")
            && handler.contains("execute_clear_storage_cache("),
        "daemon storage handlers must leave storage ownership to uc-engine"
    );
}

#[test]
fn daemon_local_device_handler_delegates_identity_to_engine() {
    let handler_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/uc-webserver/src/api/device.rs");
    let handler = std::fs::read_to_string(&handler_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler_path.display()));

    assert!(
        !handler.contains(".device.local_device_info(")
            && handler.contains("execute_query_local_device("),
        "daemon local-device handler must leave identity ownership to uc-engine"
    );
}

#[test]
fn daemon_encryption_handlers_delegate_session_ownership_to_engine() {
    let handler_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/uc-webserver/src/api/encryption.rs");
    let handler = std::fs::read_to_string(&handler_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", handler_path.display()));

    assert!(
        !handler.contains(".encryption\n        .state(")
            && !handler.contains("app.encryption.lock(")
            && !handler.contains("app.encryption.verify_keychain_access(")
            && handler.contains("execute_query_encryption_state(")
            && handler.contains("execute_lock_encryption(")
            && handler.contains("execute_verify_secure_storage_access("),
        "daemon encryption state, lock, and secure-storage handlers must delegate to uc-engine"
    );
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();

    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        {
            let path = entry
                .unwrap_or_else(|error| panic!("failed to inspect directory entry: {error}"))
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }

    sources
}
