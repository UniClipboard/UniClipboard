//! Mobile sync LAN listener — daemon 进程内的第二个 axum HTTP server,
//! 挂 SyncClipboard 兼容协议路由(根路径 `GET/PUT /SyncClipboard.json` /
//! `GET/PUT /file/:dataName`), 接受 iPhone SyncClipboard shortcut 客户端
//! 的 LAN 直连。
//!
//! 与现有 `crate::api::server`(`127.0.0.1` 动态端口的 daemon API,ADR-011)是**两个**
//! 独立 listener, 互不共享 router / 中间件。理由(SPEC §3.1):
//!
//! * daemon API 走 JWT + PID 白名单中间件;mobile LAN 走 Basic Auth(v3
//!   SyncClipboard 兼容路径), 两套鉴权语义独立。
//! * daemon API 始终绑 loopback;mobile LAN 在子步骤 5.5 接 settings 后
//!   绑用户选定的 LAN IP, 需要独立的"开 / 关"生命周期。
//!
//! 本模块与 loopback API 共享 daemon 启动的同一个 `Engine`；这里只负责
//! SyncClipboard 协议和 HTTP 生命周期，不另行装配业务运行时。

mod core;
mod middleware;
mod routes;
mod server;
mod sse_registry;

#[cfg(test)]
mod healthz_load;
#[cfg(test)]
mod test_support;

pub use server::{start_mobile_lan_server, MobileLanServerHandle};
