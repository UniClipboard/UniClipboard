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
pub fn dimension_label(dimension: crate::filters::Dimension) -> &'static str {
    use crate::filters::Dimension;
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
