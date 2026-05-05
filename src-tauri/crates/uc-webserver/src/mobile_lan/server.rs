//! `start_mobile_lan_server` —— 启动 mobile sync LAN listener。
//!
//! ## 边界
//!
//! 本函数**不感知** `MobileSyncEndpointInfoPort` —— uc-webserver 边界规则下
//! 不该直接依赖 uc-infra 的具体 adapter 类型。函数把 axum bind 后实际拿到
//! 的 `SocketAddr`(动态端口分配场景下与传入 bind 不一定相同)交回调用方,
//! 由调用方(uc-desktop daemon)负责写 `endpoint_info.set(...)`。
//!
//! ## 生命周期
//!
//! - 传入的 `cancel` 触发后,axum 走 `with_graceful_shutdown`,退出已建立
//!   的连接(≤ 5s,符合 SPEC §3.3 "graceful drain"约束)。
//! - listener 任务返回 `JoinHandle<anyhow::Result<()>>`;调用方 `await` 它
//!   并据此 `clear()` endpoint_info。

use std::net::SocketAddr;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::mobile_lan::routes::build_router;

/// `start_mobile_lan_server` 的成功返回值。
///
/// `bound_addr` 是 listener 实际拿到的地址 —— 当传入 `bind` 的 port 是 0(
/// ephemeral)时,这里能拿到真实分配的端口,daemon 据此拼出 `endpoint_info`
/// 的 URL。
pub struct MobileLanServerHandle {
    pub bound_addr: SocketAddr,
    pub join_handle: JoinHandle<anyhow::Result<()>>,
}

/// 启动 mobile sync LAN listener。
///
/// 函数会先**同步**完成 TCP bind(避免 race:调用方紧接着写 endpoint_info,
/// 这时 listener 必须已经能接受连接),再 spawn 一个 axum 任务跑事件循环。
///
/// `cancel` 触发后任务通过 `with_graceful_shutdown` 退出,`join_handle.await`
/// 返回 `Ok(())`;bind 失败 / axum 内部错误返回 `Err`。
pub async fn start_mobile_lan_server(
    bind: SocketAddr,
    cancel: CancellationToken,
) -> anyhow::Result<MobileLanServerHandle> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let bound_addr = listener.local_addr()?;
    tracing::info!(
        bound_addr = %bound_addr,
        "mobile sync LAN listener listening (Phase 3 stub: only /mobile/v1/handshake)"
    );

    let router = build_router();
    let join_handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(cancel.cancelled_owned())
            .await
            .map_err(anyhow::Error::from)
    });

    Ok(MobileLanServerHandle {
        bound_addr,
        join_handle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn loopback_ephemeral() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    }

    #[tokio::test]
    async fn binds_ephemeral_port_and_reports_actual_addr() {
        let cancel = CancellationToken::new();
        let handle = start_mobile_lan_server(loopback_ephemeral(), cancel.clone())
            .await
            .expect("bind");
        // ephemeral 0 必然被分配为非零端口。
        assert_ne!(handle.bound_addr.port(), 0);
        cancel.cancel();
        handle.join_handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn handshake_route_responds_over_tcp() {
        let cancel = CancellationToken::new();
        let handle = start_mobile_lan_server(loopback_ephemeral(), cancel.clone())
            .await
            .expect("bind");

        // 直接走 raw TCP + HTTP/1.1,避免引入 reqwest 依赖。
        let mut stream = tokio::net::TcpStream::connect(handle.bound_addr)
            .await
            .expect("connect");
        let request = format!(
            "GET /mobile/v1/handshake HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            handle.bound_addr
        );
        stream.write_all(request.as_bytes()).await.expect("write");

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.expect("read");
        let response = String::from_utf8_lossy(&buf);

        assert!(response.starts_with("HTTP/1.1 200"), "resp = {response}");
        assert!(response.contains(r#""version":"v1""#), "resp = {response}");
        assert!(
            response.contains(r#""accepts":"sha256-bearer-v1""#),
            "resp = {response}"
        );

        cancel.cancel();
        handle.join_handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cancel_drains_server_cleanly() {
        let cancel = CancellationToken::new();
        let handle = start_mobile_lan_server(loopback_ephemeral(), cancel.clone())
            .await
            .expect("bind");
        cancel.cancel();
        let result = handle.join_handle.await.expect("join");
        assert!(result.is_ok(), "graceful shutdown should return Ok");
    }
}
