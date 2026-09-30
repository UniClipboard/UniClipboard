//! User-visible text of the panel, in one place so it can move to translated resources later.
//!
//! Only text the panel itself owns lives here. Some older strings are still written inline where
//! they are used; they move here when the code around them is reworked.

pub const SEARCH_PLACEHOLDER: &str = "搜索，或输入 # 标签  @ 设备  / 类型";
pub const SUGGESTIONS: &str = "建议";
pub const ACCEPT_IN_ORDER: &str = "按顺序接受";
pub const RICH_TEXT: &str = "富文本";
pub const ALL_TYPES: &str = "全部";
pub const NO_MATCHES: &str = "暂无匹配的记录";
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
