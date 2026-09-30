//! User-visible text of the panel, in one place so it can move to translated resources later.
//!
//! Only text the panel itself owns lives here. Some older strings are still written inline where
//! they are used; they move here when the code around them is reworked.

pub const SEARCH_PLACEHOLDER: &str = "搜索，或输入 # 标签  @ 设备  / 类型";
pub const ACTIONS: &str = "操作";
pub const SEND_TO: &str = "发送到";
pub const ACTIONS_HINT: &str = "↑↓ 选择 · ⏎ 执行 · esc 返回";
pub const NOTHING_TO_OPEN: &str = "这条记录没有可打开的链接或文件。";
pub const NO_LOG_DIR: &str = "找不到日志目录。";
pub const FIRST_USE_TITLE: &str = "还没有剪贴板历史";
pub const FIRST_USE_HINT: &str = "在任意设备上复制内容，都会出现在这里。";
pub const SUMMON_ANYTIME: &str = "随时唤起";
pub const LOCKED_TITLE: &str = "应用界面已锁定";
pub const UNLOCK: &str = "解锁";
pub const DISCONNECTED_TITLE: &str = "同步服务未响应";
pub const RECONNECT_HINT: &str = "本次唤起期间恢复会自动刷新列表。";
pub const RECONNECT_NOW: &str = "立即重连";
pub const VIEW_LOGS: &str = "查看日志";
pub const CLOSE: &str = "关闭";
pub const TRY_RELAXING: &str = "试试放宽：";
pub const APPLY_SUGGESTION: &str = "应用建议";
pub const CLEAR: &str = "清空";
pub const NEEDS_APP: &str = "此功能需要在 UniClipboard 应用内使用。";
pub const HOST_GONE: &str = "无法联系主程序。";
pub const LOCKED_HINT_GUI: &str = "在主窗口输入口令解锁，解锁后这里会自动刷新。";
pub const OPEN_MAIN_WINDOW: &str = "打开主窗口";
pub const SETTINGS: &str = "设置…";
pub const SUGGESTIONS: &str = "建议";
pub const ACCEPT_IN_ORDER: &str = "按顺序接受";
pub const PRESS_AGAIN: &str = "再按";
pub const RICH_TEXT: &str = "富文本";
pub const ALL_TYPES: &str = "全部";
pub const TRY_OTHER_TERMS: &str = "试试其他关键词或筛选条件";
pub const SEARCHING: &str = "正在搜索…";
pub const JUST_NOW: &str = "刚刚";
pub const LOADING: &str = "正在加载…";
pub const LOADING_IMAGE: &str = "正在加载图片…";
pub const RETRY: &str = "重试";
pub const FIT_TO_WINDOW: &str = "完整显示";
pub const ACTUAL_SIZE: &str = "原始尺寸";
pub const DELETE_HINT_MAC: &str = "⌘⇧⌫ 删除";
pub const DELETE_HINT_OTHER: &str = "Ctrl+Shift+⌫ 删除";
pub const SHORTCUT_TAKEN: &str = "无法注册快捷键，可能已被其他程序占用。";
pub const PREVIEW_WINDOW_FAILED: &str = "无法打开预览窗口。";

/// Row under the visible filter chips when some do not fit.
pub fn hidden_count(hidden: usize) -> String {
    format!("还有 {hidden} 条")
}

/// What Enter does on a platform that cannot paste by itself.
pub const COPY: &str = "复制";

/// Display name of a content type or a built-in tag; anything else (a custom tag) is shown as is.
pub fn value_label(value: &str) -> &str {
    match value {
        "text" => "文本",
        "richtext" => RICH_TEXT,
        "image" => "图片",
        "file" => "文件",
        "link" => "链接",
        "code" => "代码",
        "favorited" => "收藏",
        "directory" => "文件夹",
        _ => value,
    }
}

/// Messages for failures of platform operations, see `ports::PlatformError`.
pub mod platform {
    pub const UNSUPPORTED: &str = "此平台暂不支持该功能。";
    pub const PANEL_WINDOW_INACCESSIBLE: &str = "无法访问面板窗口。";
    pub const PREVIEW_WINDOW_INACCESSIBLE: &str = "无法访问预览窗口。";
    pub const UNSUPPORTED_WINDOW_KIND: &str = "窗口类型不支持。";
    pub const PANEL_WINDOW_CLOSED: &str = "面板窗口已关闭。";
    pub const PREVIEW_WINDOW_CLOSED: &str = "预览窗口已关闭。";
    pub const PREVIEW_LAYER_NOT_READY: &str = "预览绘制层尚未就绪。";
    pub const MAIN_THREAD_REQUIRED: &str = "需要主线程。";
    pub const NO_DISPLAY: &str = "找不到显示器。";
    pub const INVALID_LINK: &str = "链接无效。";
    pub const CANNOT_OPEN: &str = "无法打开这项内容。";
    pub const NO_PASTE_PERMISSION: &str = "需要辅助功能权限才能自动粘贴；也可使用复制按钮。";
    pub const NO_PASTE_TARGET: &str = "没有可粘贴的目标应用，请从其他应用唤起面板。";
    pub const PASTE_TARGET_QUIT: &str = "原应用已退出，请重新唤起面板。";
    pub const FOCUS_MOVED: &str = "焦点已切换，请回到目标应用重新唤起面板。";
    pub const CANNOT_RETURN_TO_TARGET: &str = "无法切回目标应用，请重新唤起面板。";
    pub const PASTE_TARGET_MISSING: &str = "没有可粘贴的目标应用。";
    pub const CANNOT_CREATE_TYPING_EVENT: &str = "无法创建输入事件。";
    pub const CANNOT_CREATE_PASTE_EVENT: &str = "无法创建粘贴事件。";
}

/// Messages for failures of history operations, see `ports::history`.
pub mod service {
    pub const LOCKED: &str = "剪贴板已锁定，请解锁后继续。";
    pub const SEARCH_FAILED: &str = "搜索失败，请重试。";
    pub const DISCONNECTED: &str = "同步服务未响应。";
    pub const INTERRUPTED: &str = "后台请求已中断，请重试。";
    pub const SEARCH_TIMEOUT: &str = "搜索超时，请重试。";
    pub const NOT_RUNNING: &str = "无法连接 UniClipboard，请先启动并解锁桌面应用。";
    pub const CANNOT_CONNECT: &str = "无法创建后台连接。";
    pub const RESTORE_FAILED: &str = "无法复制这条记录，请刷新后重试。";
    pub const RESTORE_TIMEOUT: &str = "复制超时，请重试。";
    pub const ACTION_FAILED: &str = "操作失败，请重试。";
    pub const TAGS_UNAVAILABLE: &str = "无法读取标签。";
    pub const DEVICES_UNAVAILABLE: &str = "无法读取设备。";
    pub const PREVIEW_TIMEOUT: &str = "预览加载超时。";
    pub const PREVIEW_UNREADABLE: &str = "无法读取预览。";
    pub const ENTRY_GONE: &str = "内容已不可用。";
    pub const IMAGE_UNREADABLE: &str = "无法读取图片。";
    pub const IMAGE_GONE: &str = "图片已不可用。";
    pub const IMAGE_BAD_FORMAT: &str = "图片格式错误。";
    pub const IMAGE_UNSUPPORTED: &str = "图片格式不支持。";
    pub const IMAGE_UNDECODABLE: &str = "图片无法解码。";
}

/// Explains the disconnected page: how many times the panel has tried to reach the daemon.
pub fn reconnecting(attempt: u32) -> String {
    format!("正在自动重连（第 {attempt} 次）。")
}

/// Says what found nothing, with the typed words when there are any.
pub fn no_match(query: &str) -> String {
    if query.trim().is_empty() {
        "没有符合条件的内容".to_string()
    } else {
        format!("没有匹配“{}”的内容", query.trim())
    }
}

/// Footer text naming where the selected entry will be pasted.
pub fn paste_to(application: Option<&str>) -> String {
    match application {
        Some(name) => format!("粘贴到 {name}"),
        None => "粘贴".to_string(),
    }
}

/// Number of results shown at the right end of the search row.
pub fn result_count(total: u32) -> String {
    format!("{total} 条")
}

/// Count shown while suggestions are pending: the results are text matches only.
pub fn text_match_count(total: u32) -> String {
    format!("{total} 条文字匹配")
}

/// Heading of the text matches under the suggestions; the words that stayed plain text follow it.
pub fn text_matches_heading(words: &str) -> String {
    if words.is_empty() {
        "文字匹配".to_string()
    } else {
        format!("文字匹配 · {words}")
    }
}

/// Name of a filter dimension in a suggestion row.
pub fn dimension_label(dimension: crate::query::filters::Dimension) -> &'static str {
    use crate::query::filters::Dimension;
    match dimension {
        Dimension::Type => "类型",
        Dimension::Tag => "标签",
        Dimension::Source => "设备",
        Dimension::Time => "时间",
    }
}

/// Short age of an entry: "刚刚", "5m", "3h" or "2d".
pub fn relative_time(elapsed_ms: i64) -> String {
    let minutes = (elapsed_ms as f64 / 60_000.).round() as i64;
    if minutes < 1 {
        JUST_NOW.to_string()
    } else if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 1440 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", minutes / 1440)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_are_shown_in_the_largest_whole_unit() {
        assert_eq!(relative_time(20_000), JUST_NOW);
        assert_eq!(relative_time(-5_000), JUST_NOW);
        assert_eq!(relative_time(5 * 60_000), "5m");
        assert_eq!(relative_time(59 * 60_000), "59m");
        assert_eq!(relative_time(3 * 3_600_000), "3h");
        assert_eq!(relative_time(2 * 86_400_000), "2d");
    }

    #[test]
    fn the_footer_names_the_target_or_says_only_paste() {
        assert_eq!(paste_to(Some("Terminal")), "粘贴到 Terminal");
        assert_eq!(paste_to(None), "粘贴");
    }
}
