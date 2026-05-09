//! Slice 6 / Issue #549 · 产品 analytics — composition-root 装配。
//!
//! `compose_event_context` 是把 `uc-observability::analytics::EventContext`
//! 装配并注册到进程级全局的唯一入口。它故意**不**坐落在
//! `init_tracing_subscriber` 里——后者是同步的、运行得很早，拿不到
//! `member_repo` / `setup_status` 这些 async port，而 EventContext 需要它们
//! 才能算出 `active_device_count` 与 `space_id_hash`。本模块的位置在
//! `wire_dependencies` 之后，由各 entry 的 builder 调用一次。
//!
//! ## 字段来源对照
//!
//! | 字段 | 来源 |
//! |---|---|
//! | `anonymous_user_id` / `analytics_device_id` / `is_first_run` | `uc_observability::analytics::ids::load_or_create`（文件 IO，sync） |
//! | `app_version` | `CARGO_PKG_VERSION`（编译期） |
//! | `app_channel` | 从 `app_version` 后缀解析（`-alpha*` → Alpha / `-beta*` → Beta / 否则 Stable） |
//! | `install_source` | v1 固定 `Unknown`（暂不接 release pipeline / env） |
//! | `active_device_count` | `member_repo.list().await.len()` |
//! | `space_id_hash` | `setup_status.get_status().await.space_id` 的 SHA-256 截前 16 hex；未 setup → `None` |
//! | `os` / `os_version` / `arch` / `locale` / `timezone` | `analytics::probe`（sync） |
//!
//! ## 失败语义
//!
//! IDs / SetupStatus / member_repo 任何一项失败：本函数把错误转成
//! `tracing::warn!` 后用合理的退化值（空成员列表 → 0、setup 读不到 → 无
//! `space_id_hash`），然后**仍然**装配 EventContext 并注册。理由：
//! - telemetry 缺一个字段比缺整个 context 代价小（schema doc §4 末尾原则）；
//! - 装配失败不应让 daemon / GUI 启动失败——product analytics 是辅助通道，
//!   不能反向影响业务可用性。
//!
//! IDs 文件系统失败仍然走 `Result<()>`，因为 `load_or_create` 的失败路径
//! 通常是磁盘整盘不可写（`anonymous_user_id` 都没法落地），那是更严重的
//! 问题，应该让调用方决定要不要让进程继续。

use std::sync::Arc;

use sha2::{Digest, Sha256};
use uc_application::deps::AppDeps;
use uc_application::facade::AppPaths;
use uc_observability::analytics::{
    build_event_context, global_event_context, load_or_create_ids, set_global_event_context,
    AppChannel, EventContext, EventContextInputs, InstallSource,
};

/// 装配并注册进程级 `EventContext`。
///
/// 参见模块文档了解字段来源、失败语义、调用点设计。
///
/// ## 幂等
///
/// 已经有 context 注册时本函数直接返回 `Ok(())`，**不**触发第二次
/// `load_or_create_ids` / member_repo / setup_status 调用。理由：GUI
/// 进程内拉起 daemon 的场景（`uc-desktop::start_in_process`）会让本函数
/// 经两条路径触达——一次从 `build_gui_app` 调过来，一次从 in-process
/// daemon 的 `build_core` 调过来。如果不去重，第二次会拿到"IDs 已存在"
/// 的状态、把 `is_first_run` 翻转成 `false`，覆盖 GUI 首次启动时正确
/// 标了 `true` 的 context，丢失"首次激活"信号。
///
/// 用户重置 telemetry IDs 等显式重建场景应直接调
/// `uc_observability::analytics::set_global_event_context`，绕开本函数的
/// 幂等门控。
pub async fn compose_event_context(deps: &AppDeps, paths: &AppPaths) -> anyhow::Result<()> {
    if global_event_context().is_some() {
        tracing::debug!(
            "analytics: EventContext 已由更早的 entry 注册，本次 compose 跳过；\
             若要强制重建请直接走 `set_global_event_context`"
        );
        return Ok(());
    }

    let analytics_dir = paths.app_data_root_dir.join("analytics");
    let ids = load_or_create_ids(&analytics_dir)?;

    let active_device_count = read_active_device_count(deps).await;
    let space_id_hash = read_space_id_hash(deps).await;

    let app_version = env!("CARGO_PKG_VERSION").to_string();
    let app_channel = parse_app_channel(&app_version);

    let ctx: EventContext = build_event_context(EventContextInputs {
        anonymous_user_id: ids.anonymous_user_id,
        analytics_device_id: ids.analytics_device_id,
        app_version,
        app_channel,
        install_source: InstallSource::Unknown,
        is_first_run: ids.is_first_run,
        active_device_count,
        space_id_hash,
    });

    set_global_event_context(Arc::new(ctx));
    Ok(())
}

/// 读 `member_repo.list()` 的长度作为 `active_device_count`。
///
/// 失败 fall through 到 `0`：member_repo 不可用通常意味着 SQLite 出了问题，
/// 这种情况下 daemon / GUI 会有更显眼的报错路径处理；analytics 不放大故障。
async fn read_active_device_count(deps: &AppDeps) -> u32 {
    match deps.device.member_repo.list().await {
        Ok(members) => members.len() as u32,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "analytics: 读取 member_repo 失败，active_device_count 退化为 0"
            );
            0
        }
    }
}

/// 读 `setup_status.get_status()`，把 `space_id` 哈希成不可逆 16 hex。
///
/// schema doc §6.3：原始 `space_id` 永远不上传；上传的是 SHA-256(space_id) 的
/// 前 16 hex（64 bit），既能跨事件做"同 Space 内聚合"又不暴露原 ID。未完成
/// setup 或 `space_id` 缺失（极老安装）→ `None`，PostHog 端会落 null。
async fn read_space_id_hash(deps: &AppDeps) -> Option<String> {
    match deps.setup_status.get_status().await {
        Ok(status) => status
            .space_id
            .as_ref()
            .map(|sid| hash_space_id(sid.as_str())),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "analytics: 读取 setup_status 失败，space_id_hash 退化为 None"
            );
            None
        }
    }
}

fn hash_space_id(space_id: &str) -> String {
    let digest = Sha256::digest(space_id.as_bytes());
    let mut out = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", byte);
    }
    out
}

/// 从 `CARGO_PKG_VERSION` 后缀解析发布渠道。
///
/// 约定与项目已有 release-please 配置一致：
/// - `0.7.0-alpha.6` / `0.7.0-alpha`     → `Alpha`
/// - `0.7.0-beta.1` / `0.7.0-beta`       → `Beta`
/// - `0.7.0` / `1.0.0`                   → `Stable`
/// - 其他 prerelease（rc 等）退化为 `Alpha` 以避免误标 stable，毕竟 rc 也是
///   未 GA 的状态。
fn parse_app_channel(version: &str) -> AppChannel {
    let Some(suffix) = version.split_once('-').map(|(_, s)| s) else {
        return AppChannel::Stable;
    };
    let head = suffix.split(['.', '+']).next().unwrap_or("");
    match head {
        "alpha" => AppChannel::Alpha,
        "beta" => AppChannel::Beta,
        // rc / dev / pre / 其他都按"未 GA"处理。Stable 必须是干净的语义版本号。
        _ => AppChannel::Alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_app_channel_recognises_alpha() {
        assert_eq!(parse_app_channel("0.7.0-alpha.6"), AppChannel::Alpha);
        assert_eq!(parse_app_channel("1.0.0-alpha"), AppChannel::Alpha);
    }

    #[test]
    fn parse_app_channel_recognises_beta() {
        assert_eq!(parse_app_channel("0.8.0-beta.1"), AppChannel::Beta);
        assert_eq!(parse_app_channel("1.2.3-beta"), AppChannel::Beta);
    }

    #[test]
    fn parse_app_channel_treats_clean_semver_as_stable() {
        assert_eq!(parse_app_channel("0.7.0"), AppChannel::Stable);
        assert_eq!(parse_app_channel("1.0.0"), AppChannel::Stable);
        assert_eq!(parse_app_channel("10.20.30"), AppChannel::Stable);
    }

    #[test]
    fn parse_app_channel_falls_back_to_alpha_for_other_prerelease() {
        // rc / dev / pre 等都视为未 GA。Stable 是个强承诺，不允许擦边。
        assert_eq!(parse_app_channel("1.0.0-rc.1"), AppChannel::Alpha);
        assert_eq!(parse_app_channel("0.5.0-dev"), AppChannel::Alpha);
        assert_eq!(parse_app_channel("0.5.0-pre.1"), AppChannel::Alpha);
    }

    #[test]
    fn hash_space_id_yields_16_hex_chars() {
        let hash = hash_space_id("space-abcdef-0123");
        assert_eq!(hash.len(), 16, "schema doc §6.3：取前 16 hex");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash 必须全 hex"
        );
    }

    #[test]
    fn hash_space_id_is_deterministic() {
        let a = hash_space_id("space-xyz");
        let b = hash_space_id("space-xyz");
        assert_eq!(a, b, "同输入必须同输出，否则跨事件聚合失效");
    }

    #[test]
    fn hash_space_id_is_collision_resistant_for_distinct_inputs() {
        // 弱断言：两个不同 ID 应有不同 hash。SHA-256 截 64 bit 实际碰撞概率
        // 很低，但理论上不可能为 0；这里只验证"非平凡 hash"。
        let a = hash_space_id("space-aaa");
        let b = hash_space_id("space-bbb");
        assert_ne!(a, b);
    }
}
