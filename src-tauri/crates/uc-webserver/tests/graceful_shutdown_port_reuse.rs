//! `app.restart()` 端口让渡契约测试：axum::serve 在
//! `with_graceful_shutdown(cancel)` 触发后必须干净 drop listener，让
//! SocketAddr 立刻可在新进程上 rebind 成功。
//!
//! 方案 C (2026-05-11) 后所有"需要重启"设置走进程级 `app.restart()`
//! (Tauri spawn 新进程 + exit 当前进程)。`uc-tauri/src/commands/restart.rs`
//! 在 `app.restart()` 之前主动跑 graceful daemon shutdown,目的是让旧
//! daemon listener drop → 端口释放 → 新进程在同地址 bind 不撞
//! `WSAEADDRINUSE` (Windows os error 10048)。本测试钉死这条契约: 同进程
//! 内 cancel → rebind 同端口能立即成功。
//!
//! 历史: 该测试在 in-process daemon reload 时代 (Phase 4 上半场) 引入,
//! 当时 reload 也依赖同一条契约。方案 C 取消 in-process reload 后,
//! 契约本身对 `app.restart()` 的新旧进程交接窗口仍然成立。
//!
//! 测试只覆盖最小契约 (axum + cancel + rebind),不拉起完整 daemon 装配。

use std::net::SocketAddr;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use tokio_util::sync::CancellationToken;

/// 构造一个最简 axum 服务，绑定到 caller 指定的 addr，cancel 触发
/// graceful shutdown 后返回。返回 (实际绑定 addr, server JoinHandle)。
async fn spawn_server(
    addr: SocketAddr,
    cancel: CancellationToken,
) -> (SocketAddr, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind on requested addr must succeed");
    let bound = listener.local_addr().expect("local_addr");

    let router = Router::new().route("/health", get(|| async { "ok" }));

    let join = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .with_graceful_shutdown(cancel.cancelled_owned())
            .await
            .map_err(anyhow::Error::from)
    });

    (bound, join)
}

#[tokio::test]
async fn rebind_same_addr_after_graceful_shutdown_succeeds() {
    // 1. 拿一个 ephemeral 端口起第一轮 server。
    let cancel1 = CancellationToken::new();
    let (bound, join1) = spawn_server("127.0.0.1:0".parse().unwrap(), cancel1.clone()).await;

    // 2. 触发 graceful shutdown，等 serve task 完整退出 —— 退出意味着
    //    listener 已被 axum::serve drop（serve 持有 listener，return 时
    //    一并归还给 OS）。
    cancel1.cancel();
    let result1 = tokio::time::timeout(Duration::from_secs(5), join1)
        .await
        .expect("first server must exit promptly after cancel")
        .expect("join error");
    result1.expect("first axum::serve returned error after graceful shutdown");

    // 3. 在同一个 SocketAddr 立刻起第二轮 server。**不**等 TIME_WAIT，
    //    **不** retry —— 同进程 close + 没有 ESTABLISHED 连接残留时，
    //    OS 会立即归还端口。任何破坏这一点的改动（例如给 server bind
    //    加 SO_EXCLUSIVEADDRUSE 之类）都会让本测试 panic。
    let cancel2 = CancellationToken::new();
    let (rebound, join2) = spawn_server(bound, cancel2.clone()).await;
    assert_eq!(
        rebound, bound,
        "second bind must land on the exact port the first server held"
    );

    // 4. cleanup
    cancel2.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), join2).await;
}
