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

use uc_application::deps::AppDeps;
use uc_application::facade::AppPaths;
use uc_observability::analytics::{
    build_event_context, global_event_context, hash_space_id_for_telemetry, load_or_create_ids,
    load_space_person_id, set_global_event_context, AnalyticsPersonId, AnalyticsPort, AppChannel,
    Event, EventContext, EventContextInputs, GatedAnalyticsSink, InstallSource, NoopAnalyticsSink,
    PosthogSink, StdoutSink,
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

    // schema doc §3.4 · v2 跨设备 person 聚合：在装配 EventContext 前判断身份。
    // - 文件存在 → SpaceShared（A1 sponsor 创建过 / A2 joiner 接收过 sponsor 派发）；
    // - 文件不存在 → Solo（首次安装 / v1→v2 升级未配对 / 用户重置 telemetry）。
    //
    // 与 active_device_count / space_id_hash 同样的兜底姿态：读失败不阻塞 daemon
    // 启动，退化为 Solo（schema doc §3.3 reset 后语义）。开放问题 1（v1→v2
    // 升级）的决策 A 也走这条路：升级后 space_person_id 不存在 → Solo，直到
    // 下次 pairing 才下发新 ID。
    let analytics_person_id = match load_space_person_id(&analytics_dir) {
        Ok(Some(id)) => AnalyticsPersonId::SpaceShared(id),
        Ok(None) => AnalyticsPersonId::Solo(ids.anonymous_user_id),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "analytics: 读取 space_person_id 失败，退化为 Solo"
            );
            AnalyticsPersonId::Solo(ids.anonymous_user_id)
        }
    };

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
        analytics_person_id,
    });

    set_global_event_context(Arc::new(ctx));

    // Slice 8a / Issue #549 — Activation 漏斗起点：仅当 IDs 都是本次新生成
    // （`is_first_run = true`）时发一条 `app_first_open`。schema doc §7.1。
    //
    // 幂等门控由本函数顶部的 `global_event_context().is_some()` 守住——GUI
    // 进程内拉起 daemon 时 compose 会触达两次，第二次直接 return，绝不会
    // 重复 fire。也因此本事件捕获放在 `set_global_event_context` 之后是
    // 安全的：注册一次就跳，不存在"两次注册各 fire 一次"。
    //
    // gate 守卫由 `deps.analytics`（包了 `GatedAnalyticsSink` 一层）统一
    // 处理，本处不查 `is_analytics_enabled`——见 task_plan.md Decisions Made。
    if ids.is_first_run {
        deps.analytics.capture(Event::AppFirstOpen);
    }

    // PostHog `$pageview` / `$screen` 的桌面端等价：每次进程启动都发一次
    // `app_opened`，让 PostHog 默认 dashboard 的 DAU / WAU / MAU / 留存曲线
    // 有数据源。`AppFirstOpen` 仅首次安装触发，不足以做活跃度口径。
    //
    // 与 `AppFirstOpen` 同位置 emit，复用同一份幂等门控——每次进程启动有且
    // 仅有一次 `app_opened`，不会因 GUI 内拉起 daemon 两次 compose 而重复
    // 计数。schema doc §7.1。
    deps.analytics.capture(Event::AppOpened);

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
            .map(|sid| hash_space_id_for_telemetry(sid.as_str())),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "analytics: 读取 setup_status 失败，space_id_hash 退化为 None"
            );
            None
        }
    }
}

/// 装配进程级 [`AnalyticsPort`]。
///
/// 决策（task_plan.md Decisions Made）：sink 装一次永不替换，
/// `usage_analytics_enabled` 运行时切换由外层
/// [`GatedAnalyticsSink`] 统一守卫，不重建 sink。
///
/// - dev (`cfg!(debug_assertions)`) → `Gated(StdoutSink)`，事件镜像到
///   `tracing::debug!(target = "uc_observability::analytics")`
/// - release：
///   - 拿到 `POSTHOG_PROJECT_KEY`（运行时 env 优先 → 编译期 `option_env!` 兜底）
///     → `Gated(PosthogSink::new(key))`，事件 POST 到 PostHog Cloud US。
///   - 都没拿到 → `Gated(NoopAnalyticsSink)` + 一条 `info!`。schema doc §10：
///     产品 telemetry 是辅助通道，缺 key 不应让 daemon / GUI 启动失败；
///     `info!` 而非 `warn!` 是因为"没配 key"是合法配置（dev 自部署用户、
///     PR review 构建等场景都不应注入生产 key）。
///
/// key 注入策略与 SENTRY_DSN 同位（见 `uc-bootstrap/src/tracing.rs:155-170`）：
/// 运行时 env 让 dev / 自部署用户能覆盖；`option_env!` 让 CI release build
/// 把 secret 烤进 binary（终端用户机器上没人会设这个 env）。
pub fn build_analytics_sink() -> Arc<dyn AnalyticsPort> {
    let inner: Arc<dyn AnalyticsPort> = if cfg!(debug_assertions) {
        Arc::new(StdoutSink::new())
    } else {
        let runtime_key = std::env::var("POSTHOG_PROJECT_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        let compile_time_key = option_env!("POSTHOG_PROJECT_KEY");
        match resolve_posthog_key(runtime_key, compile_time_key) {
            Some(key) => Arc::new(PosthogSink::new(key)),
            None => {
                tracing::info!(
                    "analytics: POSTHOG_PROJECT_KEY 未配置，产品 telemetry 走 noop sink"
                );
                Arc::new(NoopAnalyticsSink)
            }
        }
    };
    Arc::new(GatedAnalyticsSink::new(inner))
}

/// 三级回退：运行时 env > 编译期 `option_env!` > `None`。
///
/// 抽出私有 fn 便于单测——`std::env::var` 与 `option_env!` 是 macro 语境，
/// 直接在 `build_analytics_sink` 内行内会导致测试无法穿透。空字符串视为
/// "未设置"（CI secret 没注入时 `${{ secrets.X }}` 会渲染成空，与"未设置"
/// 等价）。
fn resolve_posthog_key(runtime: Option<String>, compile: Option<&'static str>) -> Option<String> {
    runtime
        .filter(|s| !s.is_empty())
        .or_else(|| compile.filter(|s| !s.is_empty()).map(String::from))
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

    // hash_space_id 算法相关测试已随函数移到
    // `uc-observability::analytics::identity::tests`（覆盖：长度 / 全 hex /
    // 确定性 / 碰撞抗性）。bootstrap 这一层只验证"读 setup_status 后正确
    // hash"，与算法本身无关。

    // —— Slice 7b-3：resolve_posthog_key 三级回退 ——

    #[test]
    fn resolve_posthog_key_runtime_only() {
        let got = resolve_posthog_key(Some("phc_runtime".into()), None);
        assert_eq!(got.as_deref(), Some("phc_runtime"));
    }

    #[test]
    fn resolve_posthog_key_compile_only() {
        let got = resolve_posthog_key(None, Some("phc_compile"));
        assert_eq!(got.as_deref(), Some("phc_compile"));
    }

    #[test]
    fn resolve_posthog_key_runtime_wins_when_both_present() {
        // 运行时优先级高于编译期——dev / 自部署用户能覆盖 CI 烤进 binary 的 key。
        let got = resolve_posthog_key(Some("phc_runtime".into()), Some("phc_compile"));
        assert_eq!(got.as_deref(), Some("phc_runtime"));
    }

    #[test]
    fn resolve_posthog_key_none_when_both_missing() {
        assert_eq!(resolve_posthog_key(None, None), None);
    }

    #[test]
    fn resolve_posthog_key_treats_empty_strings_as_missing() {
        // CI 未注入 secret 时 `${{ secrets.X }}` 渲染为空字符串；compile-time
        // `option_env!` 同理。两边的空串都必须等价于"未设置"，否则会用空字符串
        // 当 api_key 调 PostHog，PostHog 会 401 把整批事件丢掉。
        assert_eq!(resolve_posthog_key(Some(String::new()), None), None);
        assert_eq!(resolve_posthog_key(None, Some("")), None);
        // 运行时空 + 编译期非空 → 退到编译期值。
        assert_eq!(
            resolve_posthog_key(Some(String::new()), Some("phc_compile")).as_deref(),
            Some("phc_compile")
        );
    }
}
