//! `InMemoryMobileSyncEndpointInfoAdapter` —— [`MobileSyncEndpointInfoPort`]
//! 的进程内实现。
//!
//! v1 daemon 没有"自我探测当前监听 URL"的可靠 OS 调用 —— bind 到 0.0.0.0
//! 后既需要从外部网卡选 IP,又需要知道实际拿到的端口（动态分配场景）。
//! 所以让 daemon 在 listener 真正起来时**主动告知**这个 adapter 它绑了哪
//! 个 LAN URL:
//!
//! 1. listener 启动 ⇒ 调 [`InMemoryMobileSyncEndpointInfoAdapter::set`]
//!    写入当前 URL(状态 = `Listening`);
//! 2. listener bind 失败 ⇒ 调 [`InMemoryMobileSyncEndpointInfoAdapter::set_bind_failure`]
//!    写入失败原因(状态 = `BindFailed{reason}`)——之前这条路径只 log 不写,
//!    导致 UI 永远只看到"未开启",失败原因无法冒到用户面前;
//! 3. listener 关闭 ⇒ 调 [`InMemoryMobileSyncEndpointInfoAdapter::clear`]
//!    擦除(状态 = `Stopped`);
//! 4. daemon 没启动 LAN listener ⇒ adapter 一开始就是 `Stopped`。
//!
//! 内部用 `tokio::sync::RwLock`:读多写极少(每次 daemon 启停才写一次,
//! 而 register / get_settings 等 use case 每个动作都会读)。
//!
//! 故意不放 OS-level 网卡探测在这里 —— 那是
//! [`crate::mobile_sync::lan_probe::NetworkInterfaceLanProbe`] 的事。

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use uc_core::mobile_sync::{LanEndpointInfo, LanListenerStatus};
use uc_core::ports::{EndpointInfoError, MobileSyncEndpointInfoPort};

pub struct InMemoryMobileSyncEndpointInfoAdapter {
    inner: RwLock<LanListenerStatus>,
}

impl Default for InMemoryMobileSyncEndpointInfoAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryMobileSyncEndpointInfoAdapter {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(LanListenerStatus::Stopped),
        }
    }

    /// daemon listener bind 成功后调用 —— 写入当前 LAN URL,状态 = Listening。
    /// 同 URL 重复设置是幂等的;不同 URL 直接覆盖(让最新的一次生效)。
    pub async fn set(&self, endpoint: LanEndpointInfo) {
        let mut guard = self.inner.write().await;
        *guard = LanListenerStatus::Listening(endpoint);
    }

    /// daemon listener bind 失败后调用 —— 写入失败原因,状态 = BindFailed。
    ///
    /// `reason` 应是面向用户的人话(典型:`std::io::Error` 经 Display 输出
    /// 的 "Address already in use" / "Cannot assign requested address" 等)。
    /// 调用方在 daemon 端用 `format!("{}", err)` 即可。
    pub async fn set_bind_failure(&self, reason: impl Into<String>) {
        let mut guard = self.inner.write().await;
        *guard = LanListenerStatus::BindFailed {
            reason: reason.into(),
        };
    }

    /// daemon listener 关闭 / 配置切换为 disabled 时调用 —— 擦除现状,
    /// 状态回到 Stopped。
    pub async fn clear(&self) {
        let mut guard = self.inner.write().await;
        *guard = LanListenerStatus::Stopped;
    }
}

#[async_trait]
impl MobileSyncEndpointInfoPort for InMemoryMobileSyncEndpointInfoAdapter {
    async fn current_status(&self) -> Result<LanListenerStatus, EndpointInfoError> {
        let guard = self.inner.read().await;
        Ok(guard.clone())
    }
}

/// 给 bootstrap 用的便捷别名 —— 把"此 adapter 同时承担 port 实现 + 写入面"
/// 这件事在类型签名上明示出来,省得调用方按原始类型来回 `as ...` 转换。
pub type SharedEndpointInfo = Arc<InMemoryMobileSyncEndpointInfoAdapter>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn defaults_to_stopped() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        assert_eq!(
            a.current_status().await.unwrap(),
            LanListenerStatus::Stopped
        );
        // 旧 default 入口仍工作,转发到 None。
        assert!(a.current_lan_endpoint().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_then_read_returns_listening() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        a.set(LanEndpointInfo {
            url: "http://192.168.1.5:42720".into(),
        })
        .await;
        match a.current_status().await.unwrap() {
            LanListenerStatus::Listening(ep) => assert_eq!(ep.url, "http://192.168.1.5:42720"),
            other => panic!("expected Listening, got {:?}", other),
        }
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
    async fn clear_resets_to_stopped() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        a.set(LanEndpointInfo {
            url: "http://192.168.1.5:42720".into(),
        })
        .await;
        a.clear().await;
        assert_eq!(
            a.current_status().await.unwrap(),
            LanListenerStatus::Stopped
        );
    }

    #[tokio::test]
    async fn bind_failure_records_reason() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        a.set_bind_failure("Address already in use (os error 48)")
            .await;
        match a.current_status().await.unwrap() {
            LanListenerStatus::BindFailed { reason } => {
                assert!(reason.contains("Address already in use"))
            }
            other => panic!("expected BindFailed, got {:?}", other),
        }
        // 旧入口在 BindFailed 下应返回 None(只有 Listening 才有 endpoint)
        assert!(a.current_lan_endpoint().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_after_bind_failure_recovers_to_listening() {
        let a = InMemoryMobileSyncEndpointInfoAdapter::new();
        a.set_bind_failure("Address already in use").await;
        // daemon 重启后 bind 成功 → 覆盖回 Listening
        a.set(LanEndpointInfo {
            url: "http://192.168.1.5:42721".into(),
        })
        .await;
        assert_eq!(
            a.current_status().await.unwrap(),
            LanListenerStatus::Listening(LanEndpointInfo {
                url: "http://192.168.1.5:42721".into()
            })
        );
    }
}
