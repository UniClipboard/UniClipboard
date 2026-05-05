//! `InMemoryMobileSyncEndpointInfoAdapter` —— [`MobileSyncEndpointInfoPort`]
//! 的进程内实现。
//!
//! v1 daemon 没有"自我探测当前监听 URL"的可靠 OS 调用 —— bind 到 0.0.0.0
//! 后既需要从外部网卡选 IP，又需要知道实际拿到的端口（动态分配场景）。
//! 所以让 daemon 在 listener 真正起来时**主动告知**这个 adapter 它绑了哪
//! 个 LAN URL：
//!
//! 1. listener 启动 ⇒ 调 [`InMemoryMobileSyncEndpointInfoAdapter::set`]
//!    写入当前 URL；
//! 2. listener 关闭 ⇒ 调 [`InMemoryMobileSyncEndpointInfoAdapter::clear`]
//!    擦除（adapter 此后报告 `None`，调用 use case 会抛
//!    `LanListenerDisabled`）；
//! 3. daemon 没启动 LAN listener ⇒ adapter 一开始就是 `None`。
//!
//! 内部用 `tokio::sync::RwLock`：读多写极少（每次 daemon 启停才写一次，
//! 而 register 等 use case 每个动作都会读）。
//!
//! 故意不放 OS-level 网卡探测在这里 —— 那是
//! [`crate::mobile_sync::lan_probe::NetworkInterfaceLanProbe`] 的事。

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use uc_core::mobile_sync::LanEndpointInfo;
use uc_core::ports::{EndpointInfoError, MobileSyncEndpointInfoPort};

#[derive(Default)]
pub struct InMemoryMobileSyncEndpointInfoAdapter {
    inner: RwLock<Option<LanEndpointInfo>>,
}

impl InMemoryMobileSyncEndpointInfoAdapter {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    /// daemon listener 启动成功后调用 —— 写入当前 LAN URL。
    /// 同 URL 重复设置是幂等的；不同 URL 直接覆盖（让最新的一次生效）。
    pub async fn set(&self, endpoint: LanEndpointInfo) {
        let mut guard = self.inner.write().await;
        *guard = Some(endpoint);
    }

    /// daemon listener 关闭 / 配置切换为 disabled 时调用 —— 擦除现状。
    pub async fn clear(&self) {
        let mut guard = self.inner.write().await;
        *guard = None;
    }
}

#[async_trait]
impl MobileSyncEndpointInfoPort for InMemoryMobileSyncEndpointInfoAdapter {
    async fn current_lan_endpoint(&self) -> Result<Option<LanEndpointInfo>, EndpointInfoError> {
        let guard = self.inner.read().await;
        Ok(guard.clone())
    }
}

/// 给 bootstrap 用的便捷别名 —— 把"此 adapter 同时承担 port 实现 + 写入面"
/// 这件事在类型签名上明示出来，省得调用方按原始类型来回 `as ...` 转换。
pub type SharedEndpointInfo = Arc<InMemoryMobileSyncEndpointInfoAdapter>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn defaults_to_none() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        assert!(a.current_lan_endpoint().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_then_read_returns_endpoint() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        a.set(LanEndpointInfo {
            url: "http://192.168.1.5:42720".into(),
        })
        .await;
        let got = a.current_lan_endpoint().await.unwrap().unwrap();
        assert_eq!(got.url, "http://192.168.1.5:42720");
    }

    #[tokio::test]
    async fn set_overrides_previous_value() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        a.set(LanEndpointInfo {
            url: "http://10.0.0.1:42720".into(),
        })
        .await;
        a.set(LanEndpointInfo {
            url: "http://192.168.1.5:42720".into(),
        })
        .await;
        let got = a.current_lan_endpoint().await.unwrap().unwrap();
        assert_eq!(got.url, "http://192.168.1.5:42720");
    }

    #[tokio::test]
    async fn clear_resets_to_none() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        a.set(LanEndpointInfo {
            url: "http://192.168.1.5:42720".into(),
        })
        .await;
        a.clear().await;
        assert!(a.current_lan_endpoint().await.unwrap().is_none());
    }
}
