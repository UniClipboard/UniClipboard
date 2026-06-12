//! Mobile sync LAN listener — daemon 进程内的第二个 axum HTTP server,
//! 挂 SyncClipboard 兼容协议路由(根路径 `GET/PUT /SyncClipboard.json` /
//! `GET/PUT /file/:dataName`), 接受 iPhone SyncClipboard shortcut 客户端
//! 的 LAN 直连。
//!
//! 与现有 `crate::api::server`(`127.0.0.1:42715` 的 daemon API)是**两个**
//! 独立 listener, 互不共享 router / 中间件。理由(SPEC §3.1):
//!
//! * daemon API 走 JWT + PID 白名单中间件;mobile LAN 走 Basic Auth(v3
//!   SyncClipboard 兼容路径), 两套鉴权语义独立。
//! * daemon API 始终绑 loopback;mobile LAN 在子步骤 5.5 接 settings 后
//!   绑用户选定的 LAN IP, 需要独立的"开 / 关"生命周期。
//!
//! 本模块依赖 `Arc<MobileSyncFacade>` 用于鉴权 + 业务路由 —— daemon 启动时
//! 已经把 facade 装配好, 这里只接受现成 Arc 注入, 不感知具体 ports。

mod middleware;
mod routes;
mod server;

#[cfg(test)]
mod test_support;

pub use server::{start_mobile_lan_server, MobileLanServerHandle};
