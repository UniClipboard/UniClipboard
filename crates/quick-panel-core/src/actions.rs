//! The action list opened with Command+K: which actions an entry offers, and their shortcuts.
//!
//! This is only data. The panel runs the actions and draws the list.

use uc_daemon_contract::api::{dto::search::SearchResultDto, types::SpaceMemberDto};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Paste,
    PastePlain,
    PasteKeepOpen,
    Copy,
    PastePaths,
    Open,
    RevealFile,
    /// Opens the list of devices to send to.
    ChooseDevice,
    /// Sends to one device, or to all of them with `None`.
    Send(Option<String>),
    Favorite(bool),
    Delete,
    OpenMainWindow,
    OpenSettings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub action: Action,
    pub label: String,
    /// The shortcut as shown, e.g. "⇧⏎".
    pub shortcut: Option<&'static str>,
    pub enabled: bool,
}

/// The Command key on macOS and Control elsewhere, in a shortcut text.
fn keys(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

fn row(
    action: Action,
    label: impl Into<String>,
    shortcut: Option<&'static str>,
    enabled: bool,
) -> Row {
    Row {
        action,
        label: label.into(),
        shortcut,
        enabled,
    }
}

/// What can be opened outside the panel: the first link, or the first file.
pub fn openable(item: &SearchResultDto) -> Option<String> {
    item.link_urls
        .first()
        .or_else(|| item.file_paths.iter().find(|path| !path.is_empty()))
        .cloned()
}

/// The actions for an entry, in display order.
pub fn rows(item: &SearchResultDto, target: Option<&str>) -> Vec<Row> {
    let usable = item.payload_state.as_deref() != Some("Lost");
    let textual = matches!(item.content_type.as_str(), "text" | "richtext");
    let is_file = item.content_type == "file";
    let favorite = item.tags.iter().any(|tag| tag == "favorited");
    let mut rows = vec![row(
        Action::Paste,
        crate::text::paste_to(target),
        Some("⏎"),
        usable,
    )];
    if textual {
        rows.push(row(Action::PastePlain, "粘贴为纯文本", Some("⇧⏎"), usable));
    }
    rows.push(row(
        Action::PasteKeepOpen,
        "粘贴并保持面板",
        Some(keys("⌘⏎", "Ctrl+⏎")),
        usable,
    ));
    rows.push(row(
        Action::Copy,
        "只复制",
        Some(keys("⌘C", "Ctrl+C")),
        usable,
    ));
    if is_file {
        rows.push(row(Action::PastePaths, "粘贴文件路径", None, usable));
    }
    if openable(item).is_some() {
        rows.push(row(Action::Open, "打开", Some(keys("⌘O", "Ctrl+O")), true));
    }
    if is_file && item.file_paths.iter().any(|path| !path.is_empty()) {
        rows.push(row(Action::RevealFile, "在文件夹中显示", None, true));
    }
    rows.push(row(Action::ChooseDevice, "发送到设备", None, usable));
    rows.push(row(
        Action::Favorite(!favorite),
        if favorite { "取消收藏" } else { "收藏" },
        None,
        true,
    ));
    rows.push(row(
        Action::OpenMainWindow,
        crate::text::OPEN_MAIN_WINDOW,
        Some(keys("⌘⇧O", "Ctrl+Shift+O")),
        true,
    ));
    rows.push(row(
        Action::Delete,
        "删除",
        Some(keys("⌘⇧⌫", "Ctrl+Shift+⌫")),
        true,
    ));
    rows.push(row(
        Action::OpenSettings,
        crate::text::SETTINGS,
        Some(keys("⌘,", "Ctrl+,")),
        true,
    ));
    rows
}

/// The devices to send to. Only connected devices can receive.
pub fn device_rows(members: &[SpaceMemberDto]) -> Vec<Row> {
    let mut rows = vec![row(
        Action::Send(None),
        "所有设备",
        None,
        !members.is_empty(),
    )];
    rows.extend(members.iter().map(|member| {
        row(
            Action::Send(Some(member.peer_id.clone())),
            member.device_name.clone(),
            None,
            member.connected,
        )
    }));
    rows
}

/// Moves a cursor over the enabled rows, wrapping at both ends.
pub fn step(rows: &[Row], from: usize, forward: bool) -> usize {
    let count = rows.len();
    (1..=count)
        .map(|offset| {
            if forward {
                (from + offset) % count
            } else {
                (from + count - offset % count) % count
            }
        })
        .find(|index| rows[*index].enabled)
        .unwrap_or(from)
}

/// The first enabled row, where the cursor starts.
pub fn first_enabled(rows: &[Row]) -> usize {
    rows.iter().position(|row| row.enabled).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(content_type: &str) -> SearchResultDto {
        SearchResultDto {
            entry_id: "e".into(),
            content_type: content_type.into(),
            active_time_ms: 0,
            tags: vec![],
            text_preview: None,
            char_count: None,
            mime_type: String::new(),
            file_extensions: vec![],
            file_names: vec![],
            file_paths: vec![],
            link_urls: vec![],
            source_device: None,
            payload_state: None,
        }
    }

    fn actions(rows: &[Row]) -> Vec<Action> {
        rows.iter().map(|row| row.action.clone()).collect()
    }

    fn member(id: &str, connected: bool) -> SpaceMemberDto {
        SpaceMemberDto {
            peer_id: id.into(),
            device_name: id.into(),
            pairing_state: "paired".into(),
            last_seen_at_ms: None,
            connected,
            channel: "direct".into(),
            connection_address: None,
        }
    }

    #[test]
    fn text_offers_plain_paste_and_no_file_or_open_actions() {
        let rows = rows(&item("text"), Some("Terminal"));
        assert_eq!(
            actions(&rows),
            [
                Action::Paste,
                Action::PastePlain,
                Action::PasteKeepOpen,
                Action::Copy,
                Action::ChooseDevice,
                Action::Favorite(true),
                Action::OpenMainWindow,
                Action::Delete,
                Action::OpenSettings,
            ]
        );
        assert_eq!(rows[0].label, "粘贴到 Terminal");
        assert!(rows.iter().all(|row| row.enabled));
    }

    #[test]
    fn a_file_can_paste_its_paths_open_and_be_revealed() {
        let mut file = item("file");
        file.file_paths = vec!["/tmp/a.txt".into()];
        let rows = rows(&file, None);
        let list = actions(&rows);
        for expected in [Action::PastePaths, Action::Open, Action::RevealFile] {
            assert!(list.contains(&expected), "{expected:?}");
        }
        assert!(!list.contains(&Action::PastePlain));
        assert_eq!(openable(&file).as_deref(), Some("/tmp/a.txt"));
    }

    #[test]
    fn a_link_opens_in_the_browser_and_a_favorite_can_be_undone() {
        let mut link = item("text");
        link.link_urls = vec!["https://example.com".into()];
        link.tags = vec!["favorited".into()];
        let rows = rows(&link, None);
        assert!(actions(&rows).contains(&Action::Open));
        assert!(actions(&rows).contains(&Action::Favorite(false)));
        assert_eq!(openable(&link).as_deref(), Some("https://example.com"));
        assert!(openable(&item("image")).is_none());
    }

    #[test]
    fn a_lost_entry_can_only_be_favorited_opened_or_deleted() {
        let mut lost = item("text");
        lost.payload_state = Some("Lost".into());
        let disabled: Vec<_> = rows(&lost, None)
            .into_iter()
            .filter(|row| !row.enabled)
            .map(|row| row.action)
            .collect();
        assert_eq!(
            disabled,
            [
                Action::Paste,
                Action::PastePlain,
                Action::PasteKeepOpen,
                Action::Copy,
                Action::ChooseDevice
            ]
        );
    }

    #[test]
    fn only_connected_devices_can_be_sent_to() {
        let rows = device_rows(&[member("phone", true), member("laptop", false)]);
        assert_eq!(rows[0].action, Action::Send(None));
        assert_eq!(
            rows.iter().map(|r| r.enabled).collect::<Vec<_>>(),
            [true, true, false]
        );
        assert!(!device_rows(&[])[0].enabled);
    }

    #[test]
    fn the_cursor_skips_disabled_rows_and_wraps() {
        let rows = device_rows(&[member("a", false), member("b", true)]);
        // Rows: all devices, a (disabled), b.
        assert_eq!(first_enabled(&rows), 0);
        assert_eq!(step(&rows, 0, true), 2);
        assert_eq!(step(&rows, 2, true), 0);
        assert_eq!(step(&rows, 0, false), 2);
        assert_eq!(step(&rows, 2, false), 0);
        let none = device_rows(&[]);
        assert_eq!(step(&none, 0, true), 0);
    }
}
