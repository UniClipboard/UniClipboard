use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{CheckMenuItem, MenuItem, Submenu};
use tauri::{Emitter, Listener, Manager};
use tokio::sync::mpsc;
use tracing::{info, warn};
use uc_daemon_client::{DaemonConnectionState, DaemonMemberClient, DaemonQueryClient};
use uc_daemon_contract::api::dto::member::MemberSyncPreferencesPatchDto;

const ITEM_PREFIX: &str = "tray.device-sync.";
const CHANGED_EVENT: &str = "devices://sync-changed";

enum Action {
    Refresh,
    SetEnabled { device_id: String, enabled: bool },
}

#[derive(Clone, PartialEq, Eq)]
struct DeviceRow {
    id: String,
    name: String,
    enabled: Option<bool>,
}

struct MenuView {
    language: String,
    submenu: Submenu<tauri::Wry>,
    items: Vec<(DeviceRow, CheckMenuItem<tauri::Wry>)>,
    placeholder: MenuItem<tauri::Wry>,
    unavailable: bool,
    pending: HashSet<String>,
}

pub(super) struct DeviceSyncMenu {
    pub submenu: Submenu<tauri::Wry>,
    view: Arc<Mutex<MenuView>>,
    sender: mpsc::UnboundedSender<Action>,
}

impl DeviceSyncMenu {
    pub fn new(app: &tauri::AppHandle, language: &str) -> tauri::Result<Self> {
        let labels = labels(language);
        let submenu = Submenu::with_id(app, "tray.devices", labels.0, true)?;
        let placeholder = MenuItem::new(app, labels.2, false, None::<&str>)?;
        submenu.append(&placeholder)?;
        let view = Arc::new(Mutex::new(MenuView {
            language: language.to_string(),
            submenu: submenu.clone(),
            items: Vec::new(),
            placeholder,
            unavailable: true,
            pending: HashSet::new(),
        }));
        let (sender, receiver) = mpsc::unbounded_channel();
        let handle = app.clone();
        let listener_sender = sender.clone();
        app.listen(CHANGED_EVENT, move |_| {
            if let Err(error) = listener_sender.send(Action::Refresh) {
                warn!(error = %error, "Device sync menu refresh unavailable");
            }
        });
        let worker_view = Arc::clone(&view);
        tauri::async_runtime::spawn(async move {
            if let Err(error) = run(&handle, worker_view, receiver).await {
                warn!(error = %error, "Device sync menu stopped");
            }
        });
        Ok(Self {
            submenu,
            view,
            sender,
        })
    }

    pub fn refresh(&self) {
        if let Err(error) = self.sender.send(Action::Refresh) {
            warn!(error = %error, "Device sync menu refresh unavailable");
        }
    }

    pub fn set_language(&self, language: &str) -> tauri::Result<()> {
        let (submenu, placeholder, unavailable) = {
            let mut view = self
                .view
                .lock()
                .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!("{e}")))?;
            view.language = language.to_string();
            (
                view.submenu.clone(),
                view.placeholder.clone(),
                view.unavailable,
            )
        };
        let labels = labels(language);
        submenu.set_text(labels.0)?;
        placeholder.set_text(if unavailable { labels.2 } else { labels.1 })?;
        Ok(())
    }

    pub fn on_menu_event(&self, id: &str) -> anyhow::Result<()> {
        let Some(device_id) = id.strip_prefix(ITEM_PREFIX) else {
            return Ok(());
        };
        let mut view = self.view.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let Some((row, item)) = view.items.iter().find(|(row, _)| row.id == device_id) else {
            return Ok(());
        };
        let Some(enabled) = row.enabled else {
            return Ok(());
        };
        // Native check items toggle automatically; retain the saved value until the request succeeds.
        item.set_checked(enabled)?;
        if view.pending.contains(device_id) {
            return Ok(());
        }
        item.set_enabled(false)?;
        if let Err(error) = self.sender.send(Action::SetEnabled {
            device_id: device_id.to_string(),
            enabled: !enabled,
        }) {
            item.set_enabled(true)?;
            return Err(error.into());
        }
        view.pending.insert(device_id.to_string());
        Ok(())
    }
}

async fn load_rows(
    query: &DaemonQueryClient,
    member: &DaemonMemberClient,
) -> anyhow::Result<Vec<DeviceRow>> {
    let devices = query.get_paired_devices().await?;
    let mut rows = Vec::with_capacity(devices.len());
    for device in devices {
        let enabled = match member.member_sync_preferences(&device.peer_id).await {
            Ok(preferences) => Some(preferences.send_enabled || preferences.receive_enabled),
            Err(error) => {
                warn!(error = %error, "Device sync preference unavailable");
                None
            }
        };
        rows.push(DeviceRow {
            id: device.peer_id,
            name: device.device_name,
            enabled,
        });
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(rows)
}

fn device_toggle_patch(enabled: bool) -> MemberSyncPreferencesPatchDto {
    MemberSyncPreferencesPatchDto {
        send_enabled: Some(enabled),
        receive_enabled: Some(enabled),
        send_content_types: None,
        receive_content_types: None,
    }
}

#[tracing::instrument(name = "tray.set_device_sync", skip(client, device_id), fields(sync_enabled = enabled))]
async fn save_device_sync(
    client: &DaemonMemberClient,
    device_id: &str,
    enabled: bool,
) -> anyhow::Result<()> {
    let result = client
        .update_member_sync_preferences(device_id, &device_toggle_patch(enabled))
        .await?;
    anyhow::ensure!(result.success, "Device sync settings update was rejected");
    info!("Device sync switch saved");
    Ok(())
}

async fn run(
    app: &tauri::AppHandle,
    view: Arc<Mutex<MenuView>>,
    mut receiver: mpsc::UnboundedReceiver<Action>,
) -> anyhow::Result<()> {
    let connection = app.state::<DaemonConnectionState>().inner().clone();
    let query = DaemonQueryClient::new(connection.clone())?;
    let member = DaemonMemberClient::new(connection)?;
    let cancellation = app
        .state::<Arc<uc_desktop::task_registry::TaskRegistry>>()
        .token()
        .clone();
    // Events provide immediate updates; periodic reconciliation also covers CLI changes and reconnects.
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let action = tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            _ = interval.tick() => Action::Refresh,
            action = receiver.recv() => match action { Some(action) => action, None => return Ok(()) },
        };
        let mut completed = None;
        if let Action::SetEnabled { device_id, enabled } = action {
            match save_device_sync(&member, &device_id, enabled).await {
                Ok(()) => {
                    if let Err(error) = app.emit(CHANGED_EVENT, &device_id) {
                        warn!(error = %error, "Failed to broadcast device sync change");
                    }
                }
                Err(error) => {
                    warn!(error = %error, error_kind = "device_sync_toggle_failed", "Failed to save device sync switch");
                    super::show_sync_error(app);
                }
            }
            completed = Some(device_id);
        }
        let rows = load_rows(&query, &member).await;
        if let Err(error) = &rows {
            warn!(error = %error, "Device sync menu unavailable");
        }
        render(app, Arc::clone(&view), rows.ok(), completed).await?;
    }
}

async fn render(
    app: &tauri::AppHandle,
    view: Arc<Mutex<MenuView>>,
    rows: Option<Vec<DeviceRow>>,
    completed: Option<String>,
) -> anyhow::Result<()> {
    let handle = app.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = (|| -> anyhow::Result<()> {
            let mut view = view.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            if let Some(device_id) = completed {
                view.pending.remove(&device_id);
            }
            view.unavailable = rows.is_none();
            let rows = rows.unwrap_or_default();
            let same_devices = view
                .items
                .iter()
                .map(|(row, _)| &row.id)
                .eq(rows.iter().map(|row| &row.id));
            if !same_devices || rows.is_empty() {
                for item in view.submenu.items()? {
                    view.submenu.remove(&item)?;
                }
                view.items.clear();
                if rows.is_empty() {
                    let labels = labels(&view.language);
                    view.placeholder.set_text(if view.unavailable {
                        labels.2
                    } else {
                        labels.1
                    })?;
                    view.submenu.append(&view.placeholder)?;
                }
                for row in rows {
                    let item = CheckMenuItem::with_id(
                        &handle,
                        format!("{ITEM_PREFIX}{}", row.id),
                        &row.name,
                        row.enabled.is_some() && !view.pending.contains(&row.id),
                        row.enabled.unwrap_or(false),
                        None::<&str>,
                    )?;
                    view.submenu.append(&item)?;
                    view.items.push((row, item));
                }
            } else {
                let pending = view.pending.clone();
                for ((previous, item), row) in view.items.iter_mut().zip(rows) {
                    item.set_text(&row.name)?;
                    item.set_checked(row.enabled.unwrap_or(false))?;
                    item.set_enabled(row.enabled.is_some() && !pending.contains(&row.id))?;
                    *previous = row;
                }
            }
            Ok(())
        })();
        let _ = sender.send(result);
    })?;
    receiver.await??;
    Ok(())
}

fn labels(language: &str) -> (&'static str, &'static str, &'static str) {
    match language {
        "zh-CN" => ("设备同步", "暂无已配对设备", "暂时无法读取设备"),
        "zh-TW" => ("裝置同步", "尚無已配對裝置", "暫時無法讀取裝置"),
        "ja-JP" => (
            "デバイスの同期",
            "ペアリング済みデバイスなし",
            "デバイスを読み込めません",
        ),
        "ru-RU" => (
            "Синхронизация устройств",
            "Нет сопряжённых устройств",
            "Устройства недоступны",
        ),
        "pt-BR" => (
            "Sincronização de dispositivos",
            "Nenhum dispositivo pareado",
            "Dispositivos indisponíveis",
        ),
        _ => ("Device Sync", "No paired devices", "Devices unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn clients() -> (MockServer, DaemonQueryClient, DaemonMemberClient) {
        let server = MockServer::start().await;
        Mock::given(path("/auth/connect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "sessionToken": "test", "expiresInSecs": 300, "refreshAtSecs": 240 }, "ts": 1
            }))).mount(&server).await;
        let connection = DaemonConnectionState::default();
        connection.set(DaemonConnectionInfo {
            base_url: server.uri(),
            ws_url: "ws://127.0.0.1/unused".into(),
            token: "test".into(),
            pid: 1,
        });
        (
            server,
            DaemonQueryClient::new(connection.clone()).unwrap(),
            DaemonMemberClient::new(connection).unwrap(),
        )
    }

    #[tokio::test]
    async fn list_includes_offline_devices_and_disables_unavailable_preferences() {
        let (server, query, member) = clients().await;
        Mock::given(path("/paired-devices"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "peerId": "b", "deviceName": "Windows", "pairingState": "Trusted", "lastSeenAtMs": null, "connected": false, "channel": "offline", "connectionAddress": null },
                    { "peerId": "a", "deviceName": "Phone", "pairingState": "Trusted", "lastSeenAtMs": null, "connected": true, "channel": "direct", "connectionAddress": null }
                ], "ts": 1
            }))).mount(&server).await;
        let content = serde_json::json!({ "text": false, "image": true, "link": true, "file": false, "codeSnippet": true, "richText": true });
        Mock::given(path("/member/b/sync-preferences"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "sendEnabled": false, "receiveEnabled": true, "sendContentTypes": content, "receiveContentTypes": content }, "ts": 1
            }))).mount(&server).await;
        Mock::given(path("/member/a/sync-preferences"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let rows = load_rows(&query, &member).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "a");
        assert_eq!(rows[0].enabled, None);
        assert_eq!(rows[1].id, "b");
        assert_eq!(rows[1].enabled, Some(true));
    }

    #[tokio::test]
    async fn save_targets_one_device_and_preserves_all_other_preferences() {
        let (server, _, member) = clients().await;
        Mock::given(method("PATCH"))
            .and(path("/member/phone/sync-preferences"))
            .and(body_json(serde_json::json!({ "sendEnabled": false, "receiveEnabled": false, "sendContentTypes": null, "receiveContentTypes": null })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": { "success": true }, "ts": 1 })))
            .expect(1).mount(&server).await;
        save_device_sync(&member, "phone", false).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "PATCH")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn rejected_save_is_not_reported_as_success() {
        let (server, _, member) = clients().await;
        Mock::given(method("PATCH"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": { "success": false }, "ts": 1 })),
            )
            .mount(&server)
            .await;
        assert!(save_device_sync(&member, "phone", true).await.is_err());
    }

    #[test]
    fn device_switch_changes_both_directions_without_overwriting_content_choices() {
        for enabled in [false, true] {
            let patch = device_toggle_patch(enabled);
            assert_eq!(patch.send_enabled, Some(enabled));
            assert_eq!(patch.receive_enabled, Some(enabled));
            assert!(patch.send_content_types.is_none());
            assert!(patch.receive_content_types.is_none());
        }
    }
}
