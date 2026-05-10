//! P1 reload 契约测试：daemon in-process reload 依赖 "axum::serve 在
//! `with_graceful_shutdown(cancel)` 触发后能干净 drop listener，让同进程
//! 立刻在同一个 SocketAddr 上 rebind 成功"。
//!
//! 该路径在 P0（`uc-desktop/src/daemon/app.rs` 的 http_handle double-poll
//! panic）修复之后被 P1 在 mobile_sync 配置变更触发的 daemon reload 路径
//! 显式依赖：旧 daemon shutdown → listener drop → 端口立即释放 → 新
//! daemon spawn 在同地址 bind 不撞 `WSAEADDRINUSE`(Windows os error 10048)。
//!
//! 测试只覆盖最小契约（axum + cancel + rebind），不拉起完整 daemon
//! 装配——daemon 全栈 reload 在 P1 落地后另写集成测试。

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
