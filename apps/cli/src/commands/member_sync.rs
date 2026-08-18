use std::io::IsTerminal;

use serde::Serialize;
use uc_daemon_client::DaemonService;
use uc_daemon_contract::api::dto::member::{
    DeviceTrustRelationshipDto, MemberSyncPreferencesDto, MemberSyncPreferencesPatchDto,
    MemberSyncResultDto,
};
use uc_daemon_contract::api::dto::settings::{ContentTypesDto, ContentTypesPatchDto};

use crate::commands::app_session::connect_facade_with_lease;
use crate::exit_codes;
use crate::{output, ui, OnOff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolveDeviceError {
    NotFound,
    DeviceIdRequired,
    AmbiguousName,
}

trait MemberSyncOperations {
    async fn update_preferences(
        &self,
        device_id: &str,
        patch: &MemberSyncPreferencesPatchDto,
    ) -> anyhow::Result<MemberSyncResultDto>;

    async fn read_preferences(&self, device_id: &str) -> anyhow::Result<MemberSyncPreferencesDto>;
}

impl<T: DaemonService + ?Sized> MemberSyncOperations for T {
    async fn update_preferences(
        &self,
        device_id: &str,
        patch: &MemberSyncPreferencesPatchDto,
    ) -> anyhow::Result<MemberSyncResultDto> {
        DaemonService::update_member_sync_preferences(self, device_id, patch).await
    }

    async fn read_preferences(&self, device_id: &str) -> anyhow::Result<MemberSyncPreferencesDto> {
        DaemonService::member_sync_preferences(self, device_id).await
    }
}

#[derive(Debug)]
enum SyncExecutionError {
    UpdateRejected,
    UpdateFailed(anyhow::Error),
    ReadFailed(anyhow::Error),
}

async fn update_and_read(
    service: &(impl MemberSyncOperations + ?Sized),
    device_id: &str,
    patch: Option<&MemberSyncPreferencesPatchDto>,
) -> Result<(&'static str, MemberSyncPreferencesDto), SyncExecutionError> {
    let status = if let Some(patch) = patch {
        match service.update_preferences(device_id, patch).await {
            Ok(result) if result.success => "updated",
            Ok(_) => return Err(SyncExecutionError::UpdateRejected),
            Err(error) => return Err(SyncExecutionError::UpdateFailed(error)),
        }
    } else {
        "current"
    };

    service
        .read_preferences(device_id)
        .await
        .map(|preferences| (status, preferences))
        .map_err(SyncExecutionError::ReadFailed)
}

fn resolve_device<'a>(
    devices: &'a [DeviceTrustRelationshipDto],
    selector: &str,
    interactive: bool,
) -> Result<&'a DeviceTrustRelationshipDto, ResolveDeviceError> {
    if let Some(device) = devices.iter().find(|device| device.device_id == selector) {
        return Ok(device);
    }
    let matches = devices
        .iter()
        .filter(|device| device.display_name.eq_ignore_ascii_case(selector))
        .collect::<Vec<_>>();
    if !interactive {
        return if matches.is_empty() {
            Err(ResolveDeviceError::NotFound)
        } else {
            Err(ResolveDeviceError::DeviceIdRequired)
        };
    }
    match matches.as_slice() {
        [] => Err(ResolveDeviceError::NotFound),
        [device] => Ok(*device),
        _ => Err(ResolveDeviceError::AmbiguousName),
    }
}

fn parse_content_types(value: &str) -> Result<ContentTypesPatchDto, String> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err("content type list must not be empty".to_string());
    }
    if values.iter().any(|value| value == "all" || value == "none") {
        if values.len() != 1 {
            return Err("`all` and `none` cannot be combined with other content types".to_string());
        }
        return Ok(content_types_patch(values[0] == "all", &[]));
    }
    const VALID: &[&str] = &["text", "image", "file", "link", "rich-text", "code-snippet"];
    if let Some(unknown) = values.iter().find(|value| !VALID.contains(&value.as_str())) {
        return Err(format!("unknown content type `{unknown}`"));
    }
    Ok(content_types_patch(false, &values))
}

fn content_types_patch(default: bool, enabled: &[String]) -> ContentTypesPatchDto {
    let has = |value: &str| default || enabled.iter().any(|item| item == value);
    ContentTypesPatchDto {
        text: Some(has("text")),
        image: Some(has("image")),
        link: Some(has("link")),
        file: Some(has("file")),
        code_snippet: Some(has("code-snippet")),
        rich_text: Some(has("rich-text")),
    }
}

fn build_patch(
    send: Option<OnOff>,
    receive: Option<OnOff>,
    send_types: Option<&str>,
    receive_types: Option<&str>,
) -> Result<MemberSyncPreferencesPatchDto, String> {
    if send.is_none() && receive.is_none() && send_types.is_none() && receive_types.is_none() {
        return Err("provide at least one sync setting to change".to_string());
    }
    Ok(MemberSyncPreferencesPatchDto {
        send_enabled: send.map(bool::from),
        receive_enabled: receive.map(bool::from),
        send_content_types: send_types.map(parse_content_types).transpose()?,
        receive_content_types: receive_types.map(parse_content_types).transpose()?,
    })
}

#[derive(Serialize)]
struct MemberSyncOutput<'a> {
    ok: bool,
    status: &'static str,
    device_id: &'a str,
    device_name: &'a str,
    send_enabled: bool,
    receive_enabled: bool,
    send_content_types: Vec<&'static str>,
    receive_content_types: Vec<&'static str>,
}

#[derive(Serialize)]
struct MemberSyncErrorOutput<'a> {
    ok: bool,
    code: &'a str,
    message: &'a str,
}

pub async fn show(device: String, json: bool, verbose: bool) -> i32 {
    run(device, None, json, verbose).await
}

pub async fn set(
    device: String,
    send: Option<OnOff>,
    receive: Option<OnOff>,
    send_types: Option<String>,
    receive_types: Option<String>,
    json: bool,
    verbose: bool,
) -> i32 {
    let patch = match build_patch(
        send,
        receive,
        send_types.as_deref(),
        receive_types.as_deref(),
    ) {
        Ok(patch) => patch,
        Err(message) => return emit_error(json, "invalid_sync_settings", &message),
    };
    run(device, Some(patch), json, verbose).await
}

async fn run(
    device: String,
    patch: Option<MemberSyncPreferencesPatchDto>,
    json: bool,
    verbose: bool,
) -> i32 {
    if !json {
        ui::header(if patch.is_some() {
            "Update member sync"
        } else {
            "Member sync"
        });
    }
    let (_lease, service) = match connect_facade_with_lease(verbose).await {
        Ok(session) => session,
        Err(code) => return code,
    };
    let trust = match service.device_trust().await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return emit_error(
                json,
                "device_trust_unavailable",
                &format!(
                    "Failed to resolve member: {}",
                    crate::commands::daemon_error_message(&err)
                ),
            )
        }
    };
    let interactive = !json && std::io::stderr().is_terminal();
    let selected = match resolve_device(&trust.devices, &device, interactive) {
        Ok(device) => device,
        Err(ResolveDeviceError::NotFound) => {
            return emit_error(json, "member_not_found", "No member matches that device.")
        }
        Err(ResolveDeviceError::DeviceIdRequired) => {
            return emit_error(
                json,
                "device_id_required",
                "Non-interactive use requires an exact device ID.",
            )
        }
        Err(ResolveDeviceError::AmbiguousName) => {
            return emit_error(
                json,
                "ambiguous_device_name",
                "More than one member has that name; use a device ID.",
            )
        }
    };

    let (status, preferences) =
        match update_and_read(service.as_ref(), &selected.device_id, patch.as_ref()).await {
            Ok(result) => result,
            Err(SyncExecutionError::UpdateRejected) => {
                return emit_error(
                    json,
                    "sync_update_rejected",
                    "The member sync settings were not updated.",
                )
            }
            Err(SyncExecutionError::UpdateFailed(err)) => {
                return emit_error(
                    json,
                    "sync_update_failed",
                    &format!(
                        "Failed to update member sync settings: {}",
                        crate::commands::daemon_error_message(&err)
                    ),
                )
            }
            Err(SyncExecutionError::ReadFailed(err)) => {
                return emit_error(
                    json,
                    "sync_read_failed",
                    &format!(
                        "Failed to read saved member sync settings: {}",
                        crate::commands::daemon_error_message(&err)
                    ),
                )
            }
        };
    emit_preferences(selected, &preferences, status, json)
}

fn emit_preferences(
    device: &DeviceTrustRelationshipDto,
    preferences: &MemberSyncPreferencesDto,
    status: &'static str,
    json: bool,
) -> i32 {
    let output = MemberSyncOutput {
        ok: true,
        status,
        device_id: &device.device_id,
        device_name: &device.display_name,
        send_enabled: preferences.send_enabled,
        receive_enabled: preferences.receive_enabled,
        send_content_types: enabled_content_types(&preferences.send_content_types),
        receive_content_types: enabled_content_types(&preferences.receive_content_types),
    };
    if json {
        return output::emit_json(&output, "member sync preferences");
    }
    ui::info("device_id", output.device_id);
    ui::info("device_name", output.device_name);
    ui::info("send", on_off(output.send_enabled));
    ui::info("receive", on_off(output.receive_enabled));
    ui::info("send_types", &output.send_content_types.join(","));
    ui::info("receive_types", &output.receive_content_types.join(","));
    ui::end(if status == "updated" {
        "Member sync settings updated."
    } else {
        "Member sync settings loaded."
    });
    exit_codes::EXIT_SUCCESS
}

fn enabled_content_types(types: &ContentTypesDto) -> Vec<&'static str> {
    let mut enabled = Vec::new();
    if types.text {
        enabled.push("text");
    }
    if types.image {
        enabled.push("image");
    }
    if types.file {
        enabled.push("file");
    }
    if types.link {
        enabled.push("link");
    }
    if types.rich_text {
        enabled.push("rich-text");
    }
    if types.code_snippet {
        enabled.push("code-snippet");
    }
    enabled
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn emit_error(json: bool, code: &str, message: &str) -> i32 {
    if json {
        output::emit_json_with_code(
            &MemberSyncErrorOutput {
                ok: false,
                code,
                message,
            },
            "member sync error",
            exit_codes::EXIT_ERROR,
        )
    } else {
        ui::error(message);
        exit_codes::EXIT_ERROR
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        build_patch, parse_content_types, resolve_device, update_and_read, MemberSyncErrorOutput,
        MemberSyncOperations, MemberSyncOutput, ResolveDeviceError, SyncExecutionError,
    };
    use crate::OnOff;
    use uc_daemon_contract::api::dto::member::{
        DeviceCompatibilityDto, DeviceGroupRelationshipDto, DeviceMembershipDto,
        DeviceReachabilityDto, DeviceSyncRelationshipDto, DeviceTrustRelationshipDto,
        MemberSyncPreferencesDto, MemberSyncPreferencesPatchDto, MemberSyncResultDto,
    };

    struct RereadFails {
        update_calls: AtomicUsize,
        read_calls: AtomicUsize,
    }

    impl MemberSyncOperations for RereadFails {
        async fn update_preferences(
            &self,
            _device_id: &str,
            _patch: &MemberSyncPreferencesPatchDto,
        ) -> anyhow::Result<MemberSyncResultDto> {
            self.update_calls.fetch_add(1, Ordering::SeqCst);
            Ok(MemberSyncResultDto { success: true })
        }

        async fn read_preferences(
            &self,
            _device_id: &str,
        ) -> anyhow::Result<MemberSyncPreferencesDto> {
            self.read_calls.fetch_add(1, Ordering::SeqCst);
            Err(anyhow::anyhow!("controlled reread failure"))
        }
    }

    fn device(id: &str, name: &str) -> DeviceTrustRelationshipDto {
        DeviceTrustRelationshipDto {
            device_id: id.to_string(),
            display_name: name.to_string(),
            is_local: false,
            reachability: DeviceReachabilityDto::Online,
            membership: DeviceMembershipDto::Active,
            group_relationship: DeviceGroupRelationshipDto::Consistent,
            compatibility: DeviceCompatibilityDto::Compatible,
            sync_relationship: DeviceSyncRelationshipDto::Usable,
            available_actions: vec![],
            blocked_reason: None,
        }
    }

    #[test]
    fn content_type_list_sets_exact_enabled_set() {
        let patch = parse_content_types("text,image").expect("content type list");
        assert_eq!(patch.text, Some(true));
        assert_eq!(patch.image, Some(true));
        assert_eq!(patch.file, Some(false));
        assert_eq!(patch.link, Some(false));
        assert_eq!(patch.rich_text, Some(false));
        assert_eq!(patch.code_snippet, Some(false));
    }

    #[test]
    fn content_type_all_and_none_are_complete_sets() {
        let all = parse_content_types("all").expect("all content types");
        assert_eq!(all.file, Some(true));
        assert_eq!(all.code_snippet, Some(true));
        let none = parse_content_types("none").expect("no content types");
        assert_eq!(none.text, Some(false));
        assert_eq!(none.rich_text, Some(false));
    }

    #[test]
    fn duplicate_content_types_are_normalized() {
        let patch = parse_content_types("text,text,image").expect("duplicate content types");
        assert_eq!(patch.text, Some(true));
        assert_eq!(patch.image, Some(true));
        assert_eq!(patch.file, Some(false));
    }

    #[test]
    fn content_type_parser_rejects_unknown_and_mixed_sentinels() {
        assert!(parse_content_types("text,video").is_err());
        assert!(parse_content_types("all,text").is_err());
        assert!(parse_content_types("").is_err());
    }

    #[test]
    fn non_interactive_resolution_requires_exact_device_id() {
        let devices = vec![device("device-a", "Laptop")];
        assert_eq!(
            resolve_device(&devices, "Laptop", false),
            Err(ResolveDeviceError::DeviceIdRequired)
        );
        assert_eq!(
            resolve_device(&devices, "device-a", false)
                .expect("exact device ID")
                .device_id,
            "device-a"
        );
        assert_eq!(
            resolve_device(&devices, "missing-device", false),
            Err(ResolveDeviceError::NotFound)
        );
    }

    #[test]
    fn interactive_resolution_rejects_ambiguous_names() {
        let devices = vec![device("device-a", "Laptop"), device("device-b", "Laptop")];
        assert_eq!(
            resolve_device(&devices, "laptop", true),
            Err(ResolveDeviceError::AmbiguousName)
        );
    }

    #[test]
    fn empty_sync_patch_is_rejected_before_request() {
        assert!(build_patch(None, None, None, None).is_err());
        let patch = build_patch(Some(OnOff::Off), None, None, None).expect("send-only patch");
        assert_eq!(patch.send_enabled, Some(false));
        assert_eq!(patch.receive_enabled, None);
    }

    #[tokio::test]
    async fn successful_update_is_not_reported_when_reread_fails() {
        let service = RereadFails {
            update_calls: AtomicUsize::new(0),
            read_calls: AtomicUsize::new(0),
        };
        let patch = build_patch(Some(OnOff::Off), None, None, None).expect("send-only patch");

        let result = update_and_read(&service, "device-a", Some(&patch)).await;

        assert!(matches!(result, Err(SyncExecutionError::ReadFailed(_))));
        assert_eq!(service.update_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.read_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn member_sync_json_shape_is_stable() {
        let value = serde_json::to_value(MemberSyncOutput {
            ok: true,
            status: "updated",
            device_id: "device-a",
            device_name: "Laptop",
            send_enabled: false,
            receive_enabled: true,
            send_content_types: vec!["text"],
            receive_content_types: vec!["text", "image"],
        })
        .expect("serialize member sync output");
        assert_eq!(value["device_id"], "device-a");
        assert_eq!(value["send_enabled"], false);
        assert_eq!(value["receive_content_types"][1], "image");
        assert!(value.get("deviceId").is_none());
    }

    #[test]
    fn member_sync_error_json_codes_are_stable() {
        for code in ["ambiguous_device_name", "sync_read_failed"] {
            let value = serde_json::to_value(MemberSyncErrorOutput {
                ok: false,
                code,
                message: "controlled failure",
            })
            .expect("serialize member sync error");
            assert_eq!(value["ok"], false);
            assert_eq!(value["code"], code);
            assert!(value.get("deviceId").is_none());
        }
    }
}
