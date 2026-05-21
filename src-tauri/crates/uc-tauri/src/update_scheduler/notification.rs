//! 更新通知的系统消息发送 + i18n labels（Phase 4A）。
//!
//! 模块只负责"发出一条新版本通知"，不负责：
//! - 何时发送（scheduler 决定）
//! - 是否去重（`LastNotifiedUpdateStore` 决定，Phase 4B 集成）
//! - 点击 handler（Phase 4D）
//!
//! 三态返回值映射到 schema doc §7.8 `update_notification_shown.delivery_status`：
//! - `Sent`：plugin `show()` 返回 Ok
//! - `PermissionDenied`：plugin 报告 `PermissionState::Denied`（mobile-only，desktop
//!   始终 `Granted`，desktop 上的权限拒绝由 OS 静默吞掉，我们只能看到 `SendFailed`）
//! - `SendFailed`：plugin `show()` 返回 Err（或 permission_state 探测失败后 show 也失败）

// Phase 4A 单独提交：函数本身在 Phase 4B 才被 scheduler 调用，期间允许 dead-code。
// Phase 4B 集成后请移除本 attribute。
#![allow(dead_code)]

use tauri::plugin::PermissionState;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tracing::warn;
use uc_observability::analytics::NotificationDeliveryStatus;

use crate::tray::normalize_language;

/// 生成更新通知的 `(title, body)` i18n 文案。
///
/// 仅支持 zh-CN / en-US 两种；其他 locale 走 en-US 兜底（与 `tray.rs` 同模式）。
/// Body 包含 version，title 不含——遵守 macOS Notification Center 习惯把版本号放在副文本。
///
/// **OQ4**：具体措辞在 PR review 期再校；当前是 grill 期草稿。
pub(crate) fn update_notification_labels(language: &str, version: &str) -> (String, String) {
    let normalized = normalize_language(language);
    match normalized {
        "zh-CN" => (
            "UniClipboard 有新版本".to_string(),
            format!("新版本 {version} 已可用，点击查看详情"),
        ),
        _ => (
            "UniClipboard update available".to_string(),
            format!("Version {version} is ready. Click to view details."),
        ),
    }
}

/// 发送系统更新通知，并返回 `delivery_status` 三态。
///
/// 调用方负责后续 `analytics.capture(Event::UpdateNotificationShown { delivery_status, .. })`。
/// 本函数不直接 emit telemetry，以便 caller 控制 properties（version / install_kind 等上下文）。
///
/// Q12.1 落地：不主动调 `request_permission()`，让 OS 在首次 `show()` 时自然弹出权限请求。
/// 仅当 plugin 报告已是 `Denied` 时短路；其他状态（Granted/Prompt/PromptWithRationale）都继续尝试 `show()`。
pub(crate) async fn send_update_notification(
    app: &AppHandle,
    language: &str,
    version: &str,
) -> NotificationDeliveryStatus {
    let (title, body) = update_notification_labels(language, version);
    let notification = app.notification();

    let permission_state = match notification.permission_state() {
        Ok(state) => state,
        Err(error) => {
            warn!(
                target: "update_scheduler",
                error = %error,
                "Notification permission probe failed; continuing to attempt show()"
            );
            PermissionState::Granted
        }
    };

    if let Some(short_circuit) = classify_permission_state(permission_state) {
        return short_circuit;
    }

    let result = notification.builder().title(title).body(body).show();
    classify_send_result(&result)
}

/// 仅当 `permission_state` 为 `Denied` 时短路返回 `PermissionDenied`；
/// 其他状态返回 `None`，调用方继续尝试 `show()`。
fn classify_permission_state(state: PermissionState) -> Option<NotificationDeliveryStatus> {
    match state {
        PermissionState::Denied => Some(NotificationDeliveryStatus::PermissionDenied),
        _ => None,
    }
}

/// 把 plugin `show()` 的 `Result` 映射到 `Sent` / `SendFailed` 两态。
fn classify_send_result<E: std::fmt::Display>(
    result: &Result<(), E>,
) -> NotificationDeliveryStatus {
    match result {
        Ok(()) => NotificationDeliveryStatus::Sent,
        Err(error) => {
            warn!(
                target: "update_scheduler",
                error = %error,
                "Failed to deliver update notification to OS",
            );
            NotificationDeliveryStatus::SendFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_zh_cn_emits_chinese() {
        let (title, body) = update_notification_labels("zh-CN", "0.12.0");
        assert_eq!(title, "UniClipboard 有新版本");
        assert_eq!(body, "新版本 0.12.0 已可用，点击查看详情");
    }

    #[test]
    fn labels_en_us_emits_english() {
        let (title, body) = update_notification_labels("en-US", "0.12.0");
        assert_eq!(title, "UniClipboard update available");
        assert_eq!(body, "Version 0.12.0 is ready. Click to view details.");
    }

    #[test]
    fn labels_zh_variants_all_normalize_to_zh_cn() {
        for input in ["zh", "zh-CN", "zh-Hans", "ZH", "Zh-Hant", "zh-TW"] {
            let (title, _body) = update_notification_labels(input, "1.0.0");
            assert_eq!(
                title, "UniClipboard 有新版本",
                "expected zh-CN labels for locale {input}"
            );
        }
    }

    #[test]
    fn labels_unknown_locale_falls_back_to_en_us() {
        for input in ["en-US", "en", "fr", "ja-JP", "", "xx"] {
            let (title, _body) = update_notification_labels(input, "1.0.0");
            assert_eq!(
                title, "UniClipboard update available",
                "expected en-US labels for locale {input}"
            );
        }
    }

    #[test]
    fn labels_body_includes_version_for_both_locales() {
        let (_title, zh_body) = update_notification_labels("zh-CN", "9.9.9-test");
        let (_title, en_body) = update_notification_labels("en-US", "9.9.9-test");
        assert!(
            zh_body.contains("9.9.9-test"),
            "zh-CN body must include version: {zh_body}"
        );
        assert!(
            en_body.contains("9.9.9-test"),
            "en-US body must include version: {en_body}"
        );
    }

    #[test]
    fn classify_permission_state_denied_short_circuits() {
        assert_eq!(
            classify_permission_state(PermissionState::Denied),
            Some(NotificationDeliveryStatus::PermissionDenied)
        );
    }

    #[test]
    fn classify_permission_state_granted_continues() {
        assert!(classify_permission_state(PermissionState::Granted).is_none());
    }

    #[test]
    fn classify_permission_state_prompt_continues() {
        // Q12.1：Prompt 状态不主动请求，留给 OS 在首次 show() 时自然弹出
        assert!(classify_permission_state(PermissionState::Prompt).is_none());
        assert!(classify_permission_state(PermissionState::PromptWithRationale).is_none());
    }

    #[test]
    fn classify_send_result_ok_is_sent() {
        let ok: Result<(), &str> = Ok(());
        assert_eq!(classify_send_result(&ok), NotificationDeliveryStatus::Sent);
    }

    #[test]
    fn classify_send_result_err_is_send_failed() {
        let err: Result<(), &str> = Err("notification daemon unavailable");
        assert_eq!(
            classify_send_result(&err),
            NotificationDeliveryStatus::SendFailed
        );
    }
}
