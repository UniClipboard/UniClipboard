//! 进程级"设备 + 应用"标签的单一来源，给所有远程/本地 sink 共享。
//!
//! ## 为什么需要这个模块
//!
//! 之前 Sentry 收到的 issue / log / span 完全没有 `device.id` / `device.role` /
//! `app.version` / `app.channel` 等设备级 meta —— 只看一条事件没法知道它来自
//! 哪台设备、是 GUI 主机还是 daemon 进程、跑的是哪个 release channel。当用户
//! 反馈"A 同步给 B 没成功"，我们在 Sentry 上根本没法把 A 端的发送事件和 B 端
//! 的接收事件 join 起来，定位跨设备问题的成本被无限放大。
//!
//! jsonl 本地日志早已通过 [`crate::context::set_global_device_id`] 在每行带上
//! `device_id`，本模块是它的远端对偶：在进程启动时一次性解析所有"稳定不变"
//! 的设备/应用字段，存在一个全局 `OnceLock` 里，[`uc-bootstrap`] 在
//! `sentry::init` 之后读取这个上下文调 `sentry::configure_scope`，让后续所有
//! 出站事件自动带上完整 meta。
//!
//! 后续 PR 还会把 `peer.device_id` / `flow.id` / `session.id` 作为 span field
//! 写到 root span 上，这是先把"设备级常量"打稳。
//!
//! ## 与 [`crate::context`] 的关系
//!
//! `context` 仍然是 jsonl formatter 读 `device_id` 的入口（避免触动现有
//! formatter 代码路径）；`scope` 是面向 Sentry / 未来 OTel 的更完整上下文。
//! 二者共存，由 `uc-bootstrap::tracing` 在启动期一次性都灌好，单次解析。

use std::env;
use std::sync::OnceLock;

/// 进程启动期一次性解析的设备 + 应用元数据。
///
/// 所有字段对单个进程的整个生命周期都是常量，因此可以用 `&'static str` /
/// `OnceLock` 表达；运行期才能确定的字段（如 `device_id`）通过显式参数注入。
#[derive(Debug, Clone)]
pub struct ScopeContext {
    /// `vault_dir/device_id.txt` 里持久化的 UUID。`None` 表示文件还没生成
    /// （首次启动且 setup 流程尚未把它写盘）。
    pub device_id: Option<String>,
    /// 同一台设备上区分多个 UniClipboard 进程的角色：
    /// `gui-host`（Tauri 主进程）/`daemon`（standalone 后台进程）/
    /// `cli`（终端入口）/`unknown`（库测试等无法判定的场景）。
    pub device_role: &'static str,
    /// OS 家族：`macos` / `linux` / `windows` ...，对应 [`std::env::consts::OS`]。
    pub platform: &'static str,
    /// `CARGO_PKG_VERSION` —— 调用方通过 [`env!`] 把*调用方自身*的版本传进来，
    /// 这样字段始终对应宿主二进制版本，而不是 uc-observability 这个 crate。
    pub app_version: &'static str,
    /// 由 CI 在 build 时通过 `APP_ENV` 注入的 release channel
    /// （`alpha` / `beta` / `production`），本地构建退化为 `dev`。
    pub app_channel: &'static str,
}

static SCOPE_CONTEXT: OnceLock<ScopeContext> = OnceLock::new();

impl ScopeContext {
    /// 用运行期拿到的 `device_id` + 调用方的 `CARGO_PKG_VERSION` 构造一份
    /// 上下文，其余字段从环境 / `std::env::consts` / `option_env!` 解析。
    ///
    /// 调用方写法：
    ///
    /// ```ignore
    /// let ctx = ScopeContext::resolve(device_id, env!("CARGO_PKG_VERSION"));
    /// ```
    pub fn resolve(device_id: Option<String>, app_version: &'static str) -> Self {
        Self {
            device_id,
            device_role: detect_role(),
            platform: env::consts::OS,
            app_version,
            app_channel: option_env!("APP_ENV")
                .filter(|s| !s.is_empty())
                .unwrap_or("dev"),
        }
    }
}

/// 装入进程级单例。首次调用成功后返回 `true`，重复调用静默返回 `false` 并
/// 保留原值——和 [`crate::context::set_global_device_id`] 的语义保持一致。
pub fn set_global_scope(ctx: ScopeContext) -> bool {
    SCOPE_CONTEXT.set(ctx).is_ok()
}

/// 读取进程级上下文；`None` 表示尚未初始化（只有早期单元测试 / 没经过
/// `uc-bootstrap` 装配路径的库 consumer 才会落到这里）。
pub fn global_scope() -> Option<&'static ScopeContext> {
    SCOPE_CONTEXT.get()
}

/// 解析当前进程的 host role。
///
/// 优先级：
///
/// 1. **`UC_HOST_ROLE` 环境变量** —— 显式覆盖。`uniclip daemon` 子命令会在
///    调 bootstrap 之前先把它设为 `daemon`，因为同一个 `uniclip` 二进制既能
///    是 CLI 又能是 daemon，光看 `current_exe` 区分不出来。
/// 2. **`current_exe()` basename** —— `uniclipboard` → `gui-host`，
///    `uniclip` → `cli`。
/// 3. 兜底 `unknown`，避免误报。
fn detect_role() -> &'static str {
    if let Ok(raw) = env::var("UC_HOST_ROLE") {
        return match raw.as_str() {
            "gui-host" => "gui-host",
            "daemon" => "daemon",
            "cli" => "cli",
            _ => "unknown",
        };
    }
    let exe = env::current_exe().ok();
    let stem = exe
        .as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    match stem {
        "uniclipboard" | "UniClipboard" => "gui-host",
        "uniclip" => "cli",
        _ => "unknown",
    }
}

/// Per-role log file stem (ADR-008 D20 P4-0).
///
/// Two co-resident processes (the GUI host and the detached `uniclipd`) must
/// not append to the same rolling log file — concurrent appends race and the
/// merged stream is unreadable. The daily appender prefixes the file with the
/// process role so each writes its own family:
///
/// - `gui-host` → `uniclipboard-gui` → `uniclipboard-gui.json.<date>`
/// - `daemon`   → `uniclipboard-daemon`
/// - `cli`      → `uniclipboard-cli`
/// - `unknown`  → `uniclipboard` (legacy base name; only lib tests / edge
///   processes land here and never run concurrently with a real process).
///
/// Resolves the role the same way [`ScopeContext::resolve`] does — via
/// [`detect_role`] (`UC_HOST_ROLE` env / `current_exe` basename) — so it is
/// correct even when called before [`set_global_scope`].
pub fn role_log_file_stem() -> &'static str {
    match detect_role() {
        "gui-host" => "uniclipboard-gui",
        "daemon" => "uniclipboard-daemon",
        "cli" => "uniclipboard-cli",
        _ => "uniclipboard",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialises tests that mutate UC_HOST_ROLE. cargo test runs tests in
    // parallel by default, and env vars are process-global — without this
    // lock the two tests below race and one observes the other's value.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_uses_supplied_device_id_and_version() {
        let ctx = ScopeContext::resolve(Some("dev-123".into()), "9.9.9");
        assert_eq!(ctx.device_id.as_deref(), Some("dev-123"));
        assert_eq!(ctx.app_version, "9.9.9");
        assert!(!ctx.platform.is_empty());
    }

    #[test]
    fn role_env_override_is_honored() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_HOST_ROLE", "daemon");
        let ctx = ScopeContext::resolve(None, "0.0.0");
        assert_eq!(ctx.device_role, "daemon");
        std::env::remove_var("UC_HOST_ROLE");
    }

    #[test]
    fn unknown_env_role_falls_back_to_unknown() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_HOST_ROLE", "bogus-value");
        let ctx = ScopeContext::resolve(None, "0.0.0");
        assert_eq!(ctx.device_role, "unknown");
        std::env::remove_var("UC_HOST_ROLE");
    }

    #[test]
    fn log_file_stem_is_role_prefixed() {
        let _guard = ENV_LOCK.lock().unwrap();
        for (role, expected) in [
            ("gui-host", "uniclipboard-gui"),
            ("daemon", "uniclipboard-daemon"),
            ("cli", "uniclipboard-cli"),
        ] {
            std::env::set_var("UC_HOST_ROLE", role);
            assert_eq!(role_log_file_stem(), expected, "role {role}");
        }
        std::env::remove_var("UC_HOST_ROLE");
    }

    #[test]
    fn log_file_stem_unknown_keeps_legacy_base() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_HOST_ROLE", "bogus-value");
        assert_eq!(role_log_file_stem(), "uniclipboard");
        std::env::remove_var("UC_HOST_ROLE");
    }
}
