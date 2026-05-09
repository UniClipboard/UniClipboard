//! 平台与运行环境探测——只用 stdlib + chrono，不引新依赖。
//!
//! 目标是把 [`super::context::EventContext`] 中"客户端跑在什么环境"这部分
//! 字段填上。探测失败一律返回 `"unknown"` 占位字符串，**不**返回错误：
//! telemetry 缺字段比 telemetry 缺事件代价小（schema doc §6 隐私契约的
//! 反面——可观测性优先）。
//!
//! ## v1 已知局限（待后续 slice 增强）
//!
//! - `detect_os_version`：当前固定返回 `"unknown"`。真实探测需要：
//!   - macOS：`sw_vers -productVersion` 或 `[NSProcessInfo operatingSystemVersion]`
//!   - Windows：`RtlGetVersion` 或 `GetVersionExW`
//!   - Linux：`/etc/os-release`
//!   通常通过 `os_info` crate 聚合。引入新依赖留给 polish 阶段。
//! - `detect_locale`：Windows 上 `LANG` 等环境变量基本不存在，会返回
//!   `"unknown"`。真实探测需要 `GetUserDefaultLocaleName`，同样留待
//!   `sys-locale` crate 引入时统一处理。
//! - `detect_timezone`：返回 UTC offset 字符串（如 `"+08:00"`），而非
//!   IANA 名（`"Asia/Shanghai"`）。Schema doc §4 已注明 v1 接受 offset。
//!   IANA 名探测需要 `iana-time-zone` crate。

use super::context::{Arch, Os};

const UNKNOWN: &str = "unknown";

/// 通过 `std::env::consts::OS` 推断操作系统。
pub fn detect_os() -> Os {
    match std::env::consts::OS {
        "macos" => Os::Macos,
        "windows" => Os::Windows,
        "linux" => Os::Linux,
        "ios" => Os::Ios,
        "android" => Os::Android,
        _ => Os::Other,
    }
}

/// 通过 `std::env::consts::ARCH` 推断 CPU 架构。
pub fn detect_arch() -> Arch {
    match std::env::consts::ARCH {
        "x86_64" => Arch::X86_64,
        "aarch64" => Arch::Aarch64,
        _ => Arch::Other,
    }
}

/// 操作系统版本——v1 stub。详见模块级文档"v1 已知局限"。
pub fn detect_os_version() -> String {
    UNKNOWN.into()
}

/// 探测用户区域。Unix 上从 `LC_ALL` / `LC_MESSAGES` / `LANG` / `LANGUAGE`
/// 环境变量按优先级取值；Windows 当前命中率低（v1 已知局限）。
///
/// 返回 BCP-47 风格字符串（`zh_CN.UTF-8` → `zh-CN`）；任何字段缺失或为
/// POSIX/C 占位都返回 `"unknown"`。
pub fn detect_locale() -> String {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Ok(raw) = std::env::var(var) {
            if let Some(locale) = normalize_locale(&raw) {
                return locale;
            }
        }
    }
    UNKNOWN.into()
}

/// 探测时区——v1 返回当前 UTC offset 字符串（如 `"+08:00"`）。
pub fn detect_timezone() -> String {
    chrono::Local::now().offset().to_string()
}

/// 把 POSIX locale 字符串归一化到 BCP-47 风格。
///
/// 规则：
/// 1. 剥离 `.charset` 后缀（`zh_CN.UTF-8` → `zh_CN`）。
/// 2. 剥离 `@modifier` 后缀（`sr_RS@latin` → `sr_RS`）。
/// 3. `_` 替换为 `-`（`zh_CN` → `zh-CN`）。
/// 4. POSIX/C 占位返回 `None`。
fn normalize_locale(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "C" || trimmed == "POSIX" {
        return None;
    }
    let without_charset = trimmed.split('.').next().unwrap_or(trimmed);
    let without_modifier = without_charset.split('@').next().unwrap_or(without_charset);
    if without_modifier.is_empty() {
        return None;
    }
    Some(without_modifier.replace('_', "-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // —— OS / Arch 全分支 ————————————————————————————————

    #[test]
    fn detect_os_returns_supported_variant_on_known_targets() {
        // 在 CI 上至少命中其中一个 known case；`Other` 是兜底，不会 panic。
        let os = detect_os();
        match std::env::consts::OS {
            "macos" => assert_eq!(os, Os::Macos),
            "windows" => assert_eq!(os, Os::Windows),
            "linux" => assert_eq!(os, Os::Linux),
            "ios" => assert_eq!(os, Os::Ios),
            "android" => assert_eq!(os, Os::Android),
            _ => assert_eq!(os, Os::Other),
        }
    }

    #[test]
    fn detect_arch_returns_supported_variant_on_known_targets() {
        let arch = detect_arch();
        match std::env::consts::ARCH {
            "x86_64" => assert_eq!(arch, Arch::X86_64),
            "aarch64" => assert_eq!(arch, Arch::Aarch64),
            _ => assert_eq!(arch, Arch::Other),
        }
    }

    // —— os_version stub ————————————————————————————————

    #[test]
    fn detect_os_version_returns_unknown_placeholder_v1() {
        assert_eq!(detect_os_version(), "unknown");
    }

    // —— locale 归一化 ————————————————————————————————

    #[test]
    fn normalize_locale_strips_charset_suffix() {
        assert_eq!(normalize_locale("zh_CN.UTF-8").as_deref(), Some("zh-CN"));
        assert_eq!(normalize_locale("en_US.UTF-8").as_deref(), Some("en-US"));
    }

    #[test]
    fn normalize_locale_strips_modifier_suffix() {
        assert_eq!(normalize_locale("sr_RS@latin").as_deref(), Some("sr-RS"));
        assert_eq!(
            normalize_locale("ca_ES.UTF-8@valencia").as_deref(),
            Some("ca-ES")
        );
    }

    #[test]
    fn normalize_locale_replaces_underscores_with_hyphens() {
        assert_eq!(normalize_locale("ja_JP").as_deref(), Some("ja-JP"));
    }

    #[test]
    fn normalize_locale_rejects_posix_and_c_placeholders() {
        assert_eq!(normalize_locale("C"), None);
        assert_eq!(normalize_locale("POSIX"), None);
        assert_eq!(normalize_locale(""), None);
        assert_eq!(normalize_locale("   "), None);
    }

    #[test]
    fn normalize_locale_handles_already_bcp47_form() {
        // 防御：理想情况下输入已经是 BCP-47，不应破坏。
        assert_eq!(normalize_locale("zh-CN").as_deref(), Some("zh-CN"));
    }

    // —— locale 端到端 ————————————————————————————————

    /// 用 `unsafe { set_var }` 操纵环境变量——Rust 1.84+ 把 `set_var` 标
    /// 为 `unsafe`（多线程下不安全）。test runner 默认并行跑测试，所以
    /// 这里**只**断言 detect_locale 在缺失环境变量时优雅退化为 `"unknown"`，
    /// 不主动改环境变量，避免与其他测试串扰。
    #[test]
    fn detect_locale_returns_string_without_panicking() {
        let value = detect_locale();
        assert!(!value.is_empty());
    }

    // —— timezone ————————————————————————————————

    #[test]
    fn detect_timezone_returns_utc_offset_string() {
        let tz = detect_timezone();
        // chrono offset 形式：`+HH:MM` / `-HH:MM` / `+00:00`，长度恒为 6。
        assert_eq!(tz.len(), 6, "timezone={tz:?}");
        assert!(
            tz.starts_with('+') || tz.starts_with('-'),
            "timezone={tz:?}"
        );
    }
}
