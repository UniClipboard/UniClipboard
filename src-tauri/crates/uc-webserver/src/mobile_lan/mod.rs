//! Mobile sync LAN listener — daemon 进程内的第二个 axum HTTP server,
//! 只挂 `/mobile/v1/*` 路由,接受 iPhone 客户端的 LAN 直连。
//!
//! 与现有 `crate::api::server`(`127.0.0.1:42715` 的 daemon API)是**两个**
//! 独立 listener,互不共享 router / 中间件。理由(SPEC §3.1):
//!
//! * daemon API 走 JWT + PID 白名单中间件;mobile LAN 走 Bearer + 签名
//!   中间件(子步骤 4 引入)。
//! * daemon API 始终绑 loopback;mobile LAN 在子步骤 5.5 接 settings 后
//!   绑用户选定的 LAN IP,需要独立的"开 / 关"生命周期。
//!
//! 本模块只负责"起 axum server + 路由"——不感知 `MobileSyncEndpointInfoPort`
//! 的写入。daemon 侧拿到 `start_mobile_lan_server` 返回的 `bound_addr` 后
//! 自己去写 `InMemoryMobileSyncEndpointInfoAdapter`。这样 uc-webserver 不
//! 需要直接依赖 uc-infra 的具体类型,边界更干净。
//!
//! Phase 3 子步骤 3:仅 stub `/mobile/v1/handshake`,无鉴权。子步骤 4 接
//! 中间件,子步骤 5 接业务路由。

mod routes;
mod server;

pub use server::{start_mobile_lan_server, MobileLanServerHandle};
